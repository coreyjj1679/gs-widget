//! Name lookup is bounded by the question length, not by the catalog size.
use super::*;

#[derive(Default)]
pub(in crate::db) struct NameIndex {
    names: HashMap<String, Vec<String>>,
    codes: HashMap<String, Vec<String>>,
    max_chars: usize,
}

impl NameIndex {
    pub(super) fn load(index: &Index) -> Self {
        let mut book = Self::default();
        let mut add = |kind: &str, id: String, names: [&str; 3]| {
            let key = format!("{kind}:{id}");
            book.codes
                .entry(id.to_lowercase())
                .or_default()
                .push(key.clone());
            book.codes
                .entry(key.to_lowercase())
                .or_default()
                .push(key.clone());
            for name in names {
                let name = terminology::normalized(&crate::db::names::plain_name(name));
                if name.is_empty() || name == "0" {
                    continue;
                }
                book.max_chars = book.max_chars.max(name.chars().count());
                let keys = book.names.entry(name).or_default();
                if !keys.contains(&key) {
                    keys.push(key.clone());
                }
            }
        };
        for row in &index.items {
            add("item", row.code_str.clone(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.heroes {
            add("hero", row.code.clone(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.monsters {
            add("monster", row.code.clone(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.npcs {
            add("npc", row.code.to_string(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.sets {
            add("set", row.code.to_string(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.quests {
            add(
                "quest",
                row.id.to_string(),
                [&row.title_en, &row.title_zh, &row.title_ko],
            );
        }
        for row in &index.recipes {
            add("recipe", row.id.to_string(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.towns {
            add("town", row.code.clone(), [&row.en, &row.zh, &row.ko]);
        }
        for row in &index.works {
            add("yard", row.code.clone(), [&row.en, &row.zh, &row.ko]);
        }
        for row in index.yards.values() {
            add("bay", row.code.clone(), [&row.en, &row.zh, &row.ko]);
        }
        book
    }

    pub(super) fn matches(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> Vec<(String, usize, usize)> {
        let include = |key: &str| {
            super::query::allowed(key, category)
                && !(category.is_none() && key.starts_with("recipe:"))
        };
        let mut matches = BTreeSet::new();
        if let Some(keys) = self.codes.get(query) {
            matches.extend(
                keys.iter()
                    .filter(|key| include(key))
                    .map(|key| (key.clone(), 0, query.len())),
            );
        }
        let chars: Vec<_> = query.char_indices().collect();
        let word = |ch: char| ch.is_ascii_alphanumeric();
        for (i, &(start, first)) in chars.iter().enumerate() {
            if word(first) && i > 0 && word(chars[i - 1].1) {
                continue;
            }
            for j in i + 1..=chars.len().min(i + self.max_chars) {
                let end = chars.get(j).map_or(query.len(), |&(pos, _)| pos);
                if j == i + 1 && !(start == 0 && end == query.len()) {
                    continue;
                }
                if word(chars[j - 1].1) && chars.get(j).is_some_and(|&(_, ch)| word(ch)) {
                    continue;
                }
                if let Some(keys) = self.names.get(&query[start..end]) {
                    matches.extend(
                        keys.iter()
                            .filter(|key| include(key))
                            .map(|key| (key.clone(), start, end)),
                    );
                }
            }
        }
        matches
            .iter()
            .filter(|(_, start, end)| {
                !matches
                    .iter()
                    .any(|(_, a, b)| a <= start && b >= end && b - a > end - start)
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_names_preserve_collisions_boundaries_and_longest_forms() {
        let mut book = NameIndex::default();
        book.names
            .insert("火石".into(), vec!["item:1".into(), "monster:M1".into()]);
        book.names.insert("覺醒火石".into(), vec!["item:2".into()]);
        book.names.insert("fire".into(), vec!["item:3".into()]);
        book.max_chars = 4;
        assert_eq!(book.matches("火石", None).len(), 2);
        assert_eq!(book.matches("火石", Some("item")).len(), 1);
        assert_eq!(
            book.matches("覺醒火石點合", None),
            vec![("item:2".into(), 0, 12)]
        );
        assert!(book.matches("firewood", None).is_empty());
        assert_eq!(book.matches("fire 點合", None).len(), 1);
    }
}
