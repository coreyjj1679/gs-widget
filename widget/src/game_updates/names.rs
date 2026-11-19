//! Compare the names stored for each language before applying display-language fallbacks.
use super::{db, Entry, GameUpdateField, Kind, Name};

const LANGUAGES: [&str; 3] = ["zh", "en", "ko"];

impl Name {
    fn raw(&self, language: &str) -> &str {
        match language {
            "zh" => &self.zh,
            "ko" => &self.ko,
            _ => &self.en,
        }
    }
}

impl Entry {
    fn names_in(&self, language: &str) -> (&str, &str) {
        let fields: &[&str] = match language {
            "zh" => &["name_zh"],
            "en" => &["name_en"],
            "ko" if self.table == "quests" => &["name_ko", "title"],
            _ => &["name_ko", "result_name_ko"],
        };
        let (old, new) = if let Some(key) = fields
            .iter()
            .find(|&&k| self.before.contains_key(k) || self.after.contains_key(k))
        {
            (
                self.before
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
                self.after
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
        } else {
            (
                self.previous_name
                    .as_ref()
                    .unwrap_or(&self.name)
                    .raw(language),
                self.name.raw(language),
            )
        };
        (present_name(old), present_name(new))
    }

    fn changed_languages(&self) -> impl Iterator<Item = &'static str> + '_ {
        LANGUAGES.into_iter().filter(|&lang| {
            let (old, new) = self.names_in(lang);
            !old.is_empty() && !new.is_empty() && old != new
        })
    }

    pub(super) fn rename_count(&self) -> usize {
        self.changed_languages().count()
    }

    pub(super) fn rename_group(&self, lang: &str) -> (String, String) {
        let ui = db::primary_lang(lang);
        let languages: Vec<_> = self.changed_languages().collect();
        let labels: Vec<_> = languages.iter().map(|l| language_label(l, ui)).collect();
        let title = labels.join(if ui == "zh" { "、" } else { ", " });
        (
            languages.join("+"),
            if title.is_empty() {
                db::ui_text(lang, "其他", "Other")
            } else {
                title
            },
        )
    }

    pub(super) fn rename(&self, lang: &str, mixed: bool) -> Rename {
        if self.kind != Kind::Renamed {
            return Rename::default();
        }
        let ui = db::primary_lang(lang);
        let mut languages = LANGUAGES;
        languages.sort_by_key(|&l| {
            if l == ui {
                0
            } else if Some(l) == db::secondary_lang(lang) {
                1
            } else {
                2
            }
        });
        let mut rename = Rename::default();
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        let mut notes = Vec::new();
        for language in languages {
            let label = language_label(language, ui);
            let (old, new) = self.names_in(language);
            match (old.is_empty(), new.is_empty()) {
                (false, false) if old != new => {
                    changed.push(label);
                    rename.changes.push(GameUpdateField {
                        label: label.into(),
                        before: old.into(),
                        after: new.into(),
                    });
                }
                (false, false) => unchanged.push(label),
                (false, true) => notes.push(match ui {
                    "zh" => format!("新版未提供{label}譯名（原名「{old}」）"),
                    "ko" => format!("새 버전에 {label} 번역 없음 (기존 이름: {old})"),
                    _ => format!("{label} translation now unavailable (previously {old})"),
                }),
                (true, false) => notes.push(match ui {
                    "zh" => format!("新增{label}譯名「{new}」"),
                    "ko" => format!("{label} 번역 추가: {new}"),
                    _ => format!("{label} translation added: {new}"),
                }),
                (true, true) => notes.push(match ui {
                    "zh" => format!("兩版均未提供{label}譯名"),
                    "ko" => format!("두 버전 모두 {label} 번역 없음"),
                    _ => format!("No {label} translation in either version"),
                }),
            }
        }
        let only = changed.len() == 1 && unchanged.len() == 2;
        let separator = if ui == "zh" { "、" } else { ", " };
        let changed = changed.join(separator);
        rename.label = match ui {
            _ if changed.is_empty() => db::ui_text(lang, "改名", "Renamed"),
            "zh" if only => format!("僅{changed}改名"),
            "zh" => format!("{changed}改名"),
            "ko" if only => format!("{changed} 이름만 변경"),
            "ko" => format!("{changed} 이름 변경"),
            _ if only => format!("{changed} name only"),
            _ => format!("{changed} rename"),
        };
        if mixed {
            rename.label.push_str(match ui {
                "zh" => " · 另有修改",
                "ko" => " · 기타 변경",
                _ => " · other changes",
            });
        }
        if !unchanged.is_empty() {
            let names = unchanged.join(separator);
            notes.insert(
                0,
                match ui {
                    "zh" => format!("{names}名稱未變"),
                    "ko" => format!("{names} 이름 유지"),
                    _ => format!("Unchanged names: {names}"),
                },
            );
        }
        rename.note = notes.join(" · ");
        rename
    }
}

fn present_name(name: &str) -> &str {
    if matches!(name.trim(), "" | "0") {
        ""
    } else {
        name
    }
}

pub(super) fn language_label(language: &str, ui: &str) -> &'static str {
    match (language, ui) {
        ("zh", "zh") => "繁中",
        ("zh", "ko") => "중국어 번체",
        ("zh", _) => "Traditional Chinese",
        ("en", "zh") => "英文",
        ("en", "ko") => "영어",
        ("en", _) => "English",
        (_, "zh") => "韓文",
        (_, "ko") => "한국어",
        _ => "Korean",
    }
}

#[derive(Default)]
pub(super) struct Rename {
    pub(super) changes: Vec<GameUpdateField>,
    pub(super) label: String,
    pub(super) note: String,
}
