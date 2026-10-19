use super::text::{lead, trail};
use super::Catalog;

impl Catalog {
    /// Return one canonical name, without the quantities, codes or metadata in a hit.
    /// `alternate` follows the secondary language displayed beside the primary name.
    pub fn copyable_name(&self, key: &str, lang: &str, alternate: bool) -> Option<String> {
        let (kind, code) = key.split_once(':')?;
        let (en, zh) = match kind {
            "item" => {
                let item = self.index.items.iter().find(|item| item.code_str == code)?;
                (&item.en, &item.zh)
            }
            "quest" => {
                let id = code.parse::<i64>().ok()?;
                let quest = self.index.quests.iter().find(|quest| quest.id == id)?;
                // The search index uses the quest code when neither title exists.
                // That placeholder is not a name and must never reach the clipboard.
                if quest.title_en.trim() == quest.code && quest.title_zh.trim() == quest.code {
                    return None;
                }
                (&quest.title_en, &quest.title_zh)
            }
            "hero" => {
                let hero = self.index.heroes.iter().find(|hero| hero.code == code)?;
                (&hero.en, &hero.zh)
            }
            "npc" => {
                let id = code.parse::<i64>().ok()?;
                let npc = self.index.npcs.iter().find(|npc| npc.code == id)?;
                (&npc.en, &npc.zh)
            }
            "monster" => {
                let monster = self.index.monsters.iter().find(|monster| monster.code == code)?;
                (&monster.en, &monster.zh)
            }
            "recipe" => {
                let id = code.parse::<i64>().ok()?;
                let recipe = self.index.recipes.iter().find(|recipe| recipe.id == id)?;
                let item = self.index.items.iter().find(|item| item.code_str == recipe.result_str)?;
                (&item.en, &item.zh)
            }
            "set" => {
                let id = code.parse::<i64>().ok()?;
                let set = self.index.sets.iter().find(|set| set.code == id)?;
                (&set.en, &set.zh)
            }
            _ => return None,
        };
        copy_language(lang, en, zh, alternate)
    }
}

fn copy_language(lang: &str, en: &str, zh: &str, alternate: bool) -> Option<String> {
    let name = if alternate { trail(lang, en, zh) } else { lead(lang, en, zh) };
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_languages_follow_display_order_and_trim_names() {
        for lang in ["zh", "zh-en"] {
            assert_eq!(copy_language(lang, " Antler ", " 鹿角 ", false).as_deref(), Some("鹿角"));
            assert_eq!(copy_language(lang, " Antler ", " 鹿角 ", true).as_deref(), Some("Antler"));
        }
        for lang in ["en", "en-zh"] {
            assert_eq!(copy_language(lang, " Antler ", " 鹿角 ", false).as_deref(), Some("Antler"));
            assert_eq!(copy_language(lang, " Antler ", " 鹿角 ", true).as_deref(), Some("鹿角"));
        }
    }

    #[test]
    fn copy_languages_fall_back_without_duplicate_or_empty_alternates() {
        for lang in ["zh-en", "en-zh"] {
            assert_eq!(copy_language(lang, "Antler", " ", false).as_deref(), Some("Antler"));
            assert_eq!(copy_language(lang, " ", "鹿角", false).as_deref(), Some("鹿角"));
            assert_eq!(copy_language(lang, "Antler", "", true), None);
            assert_eq!(copy_language(lang, "", "鹿角", true), None);
            assert_eq!(copy_language(lang, " Same ", "same", true), None);
            assert_eq!(copy_language(lang, " ", "", false), None);
            assert_eq!(copy_language(lang, " ", "", true), None);
        }
    }

    #[test]
    fn copy_resolves_canonical_names_for_every_supported_kind() {
        let catalog = Catalog::open().expect("db");
        let item = catalog.index.items.iter().find(|item| !item.en.is_empty() && !item.zh.is_empty()).expect("item");
        let quest = catalog.index.quests.iter().find(|quest| quest.title_en != quest.code && quest.title_zh != quest.code).expect("quest");
        let hero = catalog.index.heroes.iter().find(|hero| !hero.en.is_empty()).expect("hero");
        let npc = catalog.index.npcs.iter().find(|npc| !npc.en.is_empty()).expect("npc");
        let monster = catalog.index.monsters.iter().find(|monster| !monster.en.is_empty()).expect("monster");
        let set = catalog.index.sets.iter().find(|set| !set.en.is_empty()).expect("set");
        let recipe = catalog.index.recipes.first().expect("recipe");
        let result = catalog.index.items.iter().find(|item| item.code_str == recipe.result_str).expect("recipe result");
        let cases = [
            (format!("item:{}", item.code_str), &item.en, &item.zh),
            (format!("quest:{}", quest.id), &quest.title_en, &quest.title_zh),
            (format!("hero:{}", hero.code), &hero.en, &hero.zh),
            (format!("npc:{}", npc.code), &npc.en, &npc.zh),
            (format!("monster:{}", monster.code), &monster.en, &monster.zh),
            (format!("set:{}", set.code), &set.en, &set.zh),
            (format!("recipe:{}", recipe.id), &result.en, &result.zh),
        ];
        for (key, en, zh) in cases {
            for lang in ["en-zh", "zh-en"] {
                for alternate in [false, true] {
                    assert_eq!(catalog.copyable_name(&key, lang, alternate), copy_language(lang, en, zh, alternate), "{key} {lang} {alternate}");
                }
            }
        }
    }

    #[test]
    fn copy_rejects_decorated_missing_and_synthetic_keys() {
        let mut catalog = Catalog::open().expect("db");
        let item = catalog.index.items.first_mut().expect("item");
        item.en = "Antler".into();
        item.zh = "鹿角".into();
        let key = format!("item:{}", item.code_str);
        assert_eq!(catalog.copyable_name(&key, "en-zh", false).as_deref(), Some("Antler"));
        for invalid in [
            format!("{key} × 3"), format!("{key}:extra"), "item:-999999".into(),
            "quest:1 × 2".into(), "quest-group:main".into(), "patch:V3.4220:live".into(),
            "nation:1".into(), "qsrc:main".into(), "work:1".into(), "Antler × 3".into(), String::new(),
        ] {
            assert_eq!(catalog.copyable_name(&invalid, "en-zh", false), None, "{invalid}");
        }
        let quest = catalog.index.quests.first_mut().expect("quest");
        quest.title_en = quest.code.clone();
        quest.title_zh = quest.code.clone();
        let quest_key = format!("quest:{}", quest.id);
        assert_eq!(catalog.copyable_name(&quest_key, "en-zh", false), None);
        assert_eq!(catalog.copyable_name(&quest_key, "zh-en", true), None);
    }
}
