//! Read and validate an optional external vocabulary without modifying it.
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::{normalized, Evidence, Glossary};

const MAX_BYTES: u64 = 1024 * 1024;

/// Path selection and fallback belong to the caller. A selected file is either
/// accepted in full or returned as an error; individual entries are never lost
/// silently. The supplied lookup checks targets against the active catalog.
pub(crate) fn load_external(
    path: &Path,
    valid_key: impl Fn(&str) -> bool,
) -> Result<Glossary, String> {
    let read = || -> Result<Glossary, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("expected a regular JSON file".into());
        }
        if metadata.len() > MAX_BYTES {
            return Err("file exceeds the 1 MiB limit".into());
        }
        let file = File::open(path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        // Also bound the read if the file grows after its metadata was checked.
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("file exceeds the 1 MiB limit".into());
        }
        decode(&bytes, &valid_key)
    };
    read().map_err(|error| format!("Invalid terminology file {}: {error}", path.display()))
}

fn decode(bytes: &[u8], valid_key: &impl Fn(&str) -> bool) -> Result<Glossary, String> {
    let book: Glossary = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    validate(&book, valid_key)?;
    Ok(book)
}

fn nonempty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn aliases<'a>(values: impl Iterator<Item = &'a String>, field: &str) -> Result<(), String> {
    let mut count = 0;
    for value in values {
        if normalized(value).is_empty() || value.chars().any(char::is_control) {
            return Err(format!(
                "{field} contains an empty alias or control character"
            ));
        }
        count += 1;
    }
    if count == 0 {
        return Err(format!("{field} must contain at least one alias"));
    }
    Ok(())
}

fn evidence(entries: &[Evidence], field: &str) -> Result<(), String> {
    if entries.is_empty() {
        return Err(format!("{field} must contain at least one source"));
    }
    for (index, entry) in entries.iter().enumerate() {
        let field = format!("{field}[{index}]");
        nonempty(&entry.source, &format!("{field}.source"))?;
        nonempty(&entry.reference, &format!("{field}.reference"))?;
        nonempty(&entry.note, &format!("{field}.note"))?;
        if !entry.spellings.is_empty() {
            aliases(entry.spellings.iter(), &format!("{field}.spellings"))?;
        }
    }
    Ok(())
}

fn typed_key(key: &str) -> bool {
    let Some((kind, id)) = key.split_once(':') else {
        return false;
    };
    if id.is_empty() || key.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return false;
    }
    match kind {
        "item" | "recipe" | "quest" | "npc" | "set" => {
            id.bytes().all(|ch| ch.is_ascii_digit()) && id.parse::<i64>().is_ok_and(|id| id > 0)
        }
        // Companion IDs include a second colon, e.g. hero:mount:3.
        "hero" | "monster" => true,
        _ => false,
    }
}

fn validate(book: &Glossary, valid_key: &impl Fn(&str) -> bool) -> Result<(), String> {
    if book.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}; expected 1",
            book.schema_version
        ));
    }
    nonempty(&book.description, "description")?;
    nonempty(&book.acquisition_note, "acquisition_note")?;
    let mut ids = HashSet::new();
    for (index, term) in book.entities.iter().enumerate() {
        let field = format!("entities[{index}]");
        nonempty(&term.id, &format!("{field}.id"))?;
        if !ids.insert(term.id.as_str()) {
            return Err(format!("duplicate term ID {:?}", term.id));
        }
        aliases(
            term.aliases
                .iter()
                .chain(&term.exact_aliases)
                .chain(&term.subject_aliases),
            &format!("{field} ({}) aliases", term.id),
        )?;
        for (language, name) in [
            ("zh", &term.canonical_names.zh),
            ("en", &term.canonical_names.en),
            ("ko", &term.canonical_names.ko),
        ] {
            nonempty(name, &format!("{field}.canonical_names.{language}"))?;
        }
        evidence(&term.evidence, &format!("{field}.evidence"))?;
        if term.keys.is_empty() {
            return Err(format!("{field}.keys must contain at least one target"));
        }
        let mut targets = HashSet::new();
        for key in &term.keys {
            if !typed_key(key) {
                return Err(format!(
                    "{field}.keys contains an invalid typed target {key:?}"
                ));
            }
            if !targets.insert(key) {
                return Err(format!("{field}.keys repeats target {key:?}"));
            }
            if !valid_key(key) {
                return Err(format!(
                    "{field}.keys target does not exist in the active catalog: {key}"
                ));
            }
        }
    }
    for (index, term) in book.intents.iter().enumerate() {
        let field = format!("intents[{index}]");
        nonempty(&term.id, &format!("{field}.id"))?;
        if !ids.insert(term.id.as_str()) {
            return Err(format!("duplicate term ID {:?}", term.id));
        }
        nonempty(&term.topic, &format!("{field}.topic"))?;
        aliases(
            term.aliases.iter().chain(&term.exact_aliases),
            &format!("{field} ({}) aliases", term.id),
        )?;
        evidence(&term.evidence, &format!("{field}.evidence"))?;
    }
    // Do not merge entries sharing a spelling. Distinct term IDs deliberately
    // survive so the resolver can present the existing ambiguous result.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../../data/terminology.json")).unwrap()
    }

    fn parse(value: &Value) -> Result<Glossary, String> {
        decode(&serde_json::to_vec(value).unwrap(), &|_| true)
    }

    struct Temporary(PathBuf);

    impl Temporary {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "gs-widget-terminology-{}-{}.json",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .unwrap();
            file.write_all(bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn external_vocabulary_loads_without_changing_the_file() {
        let value = fixture();
        let bytes = serde_json::to_vec(&value).unwrap();
        let file = Temporary::new(&bytes);
        let allowed: HashSet<_> = value["entities"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|term| term["keys"].as_array().unwrap())
            .map(|key| key.as_str().unwrap())
            .collect();
        let book = load_external(&file.0, |key| allowed.contains(key)).unwrap();
        assert_eq!(
            book.entities.len(),
            value["entities"].as_array().unwrap().len()
        );
        assert_eq!(std::fs::read(&file.0).unwrap(), bytes);
    }

    #[test]
    fn malformed_files_return_path_and_parse_errors() {
        let file = Temporary::new(b"{not json}");
        let error = load_external(&file.0, |_| true).unwrap_err();
        assert!(error.contains(file.0.to_str().unwrap()));
        assert!(error.contains("line 1"));
        assert!(load_external(&std::env::temp_dir(), |_| true)
            .unwrap_err()
            .contains("regular JSON file"));
        let mut value = fixture();
        value["schema_version"] = json!(2);
        assert!(parse(&value).unwrap_err().contains("schema_version"));
        value["schema_version"] = json!(1);
        value["intents"][0]["intent"] = json!("unknown_action");
        assert!(parse(&value).unwrap_err().contains("unknown variant"));
    }

    #[test]
    fn files_over_one_mebibyte_are_rejected_before_parsing() {
        let file = Temporary::new(b"{}");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&file.0)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(load_external(&file.0, |_| true)
            .unwrap_err()
            .contains("1 MiB"));
    }

    #[test]
    fn malformed_entity_fields_and_missing_targets_are_rejected() {
        for (field, invalid) in [
            ("id", json!(" ")),
            ("aliases", json!([""])),
            ("keys", json!([])),
            ("evidence", json!([])),
        ] {
            let mut value = fixture();
            value["entities"][0][field] = invalid;
            assert!(parse(&value).is_err(), "accepted empty {field}");
        }
        let mut value = fixture();
        value["entities"][0]["canonical_names"]["ko"] = json!("");
        assert!(parse(&value).unwrap_err().contains("canonical_names.ko"));
        let mut value = fixture();
        value["entities"][0]["evidence"][0]["reference"] = json!("");
        assert!(parse(&value).unwrap_err().contains("reference"));
        let error = decode(&serde_json::to_vec(&fixture()).unwrap(), &|key| {
            key != "hero:T49"
        })
        .unwrap_err();
        assert!(error.contains("active catalog: hero:T49"));
        for key in [
            "T49",
            "hero:",
            "place:1",
            "item:no",
            "item:-1",
            "npc:0",
            "hero:T 49",
        ] {
            let mut value = fixture();
            value["entities"][0]["keys"] = json!([key]);
            assert!(
                parse(&value).unwrap_err().contains("invalid typed target"),
                "accepted {key}"
            );
        }
        let mut value = fixture();
        value["entities"][0]["keys"] = json!(["hero:mount:3"]);
        assert!(parse(&value).is_ok());
    }

    #[test]
    fn duplicate_ids_fail_but_shared_aliases_preserve_ambiguity() {
        let mut value = fixture();
        value["entities"][1]["id"] = value["entities"][0]["id"].clone();
        assert!(parse(&value).unwrap_err().contains("duplicate term ID"));
        let mut value = fixture();
        value["intents"][0]["id"] = value["entities"][0]["id"].clone();
        assert!(parse(&value).unwrap_err().contains("duplicate term ID"));
        let mut value = fixture();
        value["entities"][1]["aliases"] = value["entities"][0]["aliases"].clone();
        let book = parse(&value).expect("shared aliases remain separate candidates");
        assert_eq!(book.entities[0].aliases, book.entities[1].aliases);
        assert_ne!(book.entities[0].keys, book.entities[1].keys);
    }

    #[test]
    fn malformed_intent_fields_are_rejected() {
        for (field, invalid) in [
            ("id", json!("")),
            ("topic", json!(" ")),
            ("aliases", json!(["\u{0000}"])),
            ("evidence", json!([])),
        ] {
            let mut value = fixture();
            value["intents"][0][field] = invalid;
            assert!(parse(&value).is_err(), "accepted malformed intent {field}");
        }
        let mut value = fixture();
        value["intents"][0]["aliases"] = json!([]);
        value["intents"][0]["exact_aliases"] = json!([]);
        assert!(parse(&value).unwrap_err().contains("at least one alias"));
    }
}
