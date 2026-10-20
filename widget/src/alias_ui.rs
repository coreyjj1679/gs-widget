use slint::{Image, ModelRc, VecModel};

use super::db;
use super::agf;
use super::icons::image_from_icon;
use super::{AliasRow, App, Hit};

#[cfg(test)]
mod tests {
    use super::{alias_code_list, alias_text, validated_alias_text, AliasIssue};

    #[test]
    fn alias_codes_split_on_commas_and_blank_rows_drop_out() {
        assert_eq!(alias_code_list("6213, 6214，6215"), vec!["6213", "6214", "6215"]);
        assert_eq!(
            alias_text(&[(" SSA ".into(), "6213 6214".into()), (String::new(), "1".into())]),
            "SSA 6213 6214"
        );
    }

    #[test]
    fn invalid_alias_names_cannot_silently_disappear_or_become_item_codes() {
        for name in ["", "A", "天", "123", "two words", "two,words", "two，words", "#SSA", "item:123"] {
            let rows = vec![(name.into(), "6213".into())];
            let error = validated_alias_text(&rows, |_| true).unwrap_err();
            assert_eq!(error.row, 0, "{name}");
            assert_eq!(error.issue, AliasIssue::Name, "{name}");
            assert_eq!(rows[0].0, name, "validation must preserve the draft");
        }
    }

    #[test]
    fn aliases_require_valid_existing_items_and_keep_the_failing_row() {
        for (codes, issue) in [
            ("", AliasIssue::Items),
            ("6213 typo", AliasIssue::Code("typo".into())),
            ("-1", AliasIssue::Code("-1".into())),
            ("999999999999999999999999", AliasIssue::Code("999999999999999999999999".into())),
            ("9000", AliasIssue::Unknown("9000".into())),
        ] {
            let rows = vec![("SSA".into(), "6213".into()), ("Test".into(), codes.into())];
            let error = validated_alias_text(&rows, |code| code == "6213").unwrap_err();
            assert_eq!(error.row, 1);
            assert_eq!(error.issue, issue);
            assert_eq!(rows[1].1, codes, "validation must preserve the draft");
        }
        assert_eq!(validated_alias_text(&[(" SSA ".into(), "#6213, 6214，6213".into())], |_| true).unwrap(), "SSA 6213 6214");
        assert_eq!(validated_alias_text(&[], |_| false).unwrap(), "");
    }
}

#[derive(Debug, PartialEq)]
enum AliasIssue {
    Name,
    Items,
    Code(String),
    Unknown(String),
}

#[derive(Debug)]
pub(crate) struct AliasValidationError {
    pub row: usize,
    issue: AliasIssue,
}

impl AliasValidationError {
    pub fn show(&self, app: &App) {
        let row = self.row + 1;
        let (zh, en, ko) = match &self.issue {
            AliasIssue::Name => (
                "別名須至少 2 個字元，不能只含數字或包含空白、逗號、#、冒號。".to_string(),
                "Use at least 2 characters for the alias, without spaces, commas, # or colons. It cannot be only numbers.".to_string(),
                "별명은 2자 이상이어야 하며 숫자만 사용하거나 공백, 쉼표, #, 콜론을 포함할 수 없습니다.".to_string(),
            ),
            AliasIssue::Items => (
                "請搜尋並加入至少一個道具。".to_string(),
                "Search for and add at least one item.".to_string(),
                "아이템을 검색하여 하나 이상 추가해 주세요.".to_string(),
            ),
            AliasIssue::Code(code) => (
                format!("「{code}」不是有效道具編號。請使用數字，或從搜尋結果選擇道具。"),
                format!("“{code}” is not a valid item ID. Use numbers or select an item from search results."),
                format!("“{code}”은(는) 올바른 아이템 번호가 아닙니다. 숫자를 입력하거나 검색 결과에서 선택해 주세요."),
            ),
            AliasIssue::Unknown(code) => (
                format!("找不到道具 #{code}。請檢查編號，或從搜尋結果選擇道具。"),
                format!("Item #{code} was not found. Check the ID or select an item from search results."),
                format!("아이템 #{code}을(를) 찾을 수 없습니다. 번호를 확인하거나 검색 결과에서 선택해 주세요."),
            ),
        };
        crate::feedback::error_for(app, "aliases", &format!("第 {row} 個別名：{zh}"), &format!("Alias {row}: {en}"), &format!("별명 {row}: {ko}"));
    }
}

/// Validate the complete draft before serialization; the legacy text format
/// otherwise silently drops incomplete rows or interprets names as item codes.
pub(crate) fn validated_alias_text(
    rows: &[(String, String)],
    mut known_item: impl FnMut(&str) -> bool,
) -> Result<String, AliasValidationError> {
    let mut normalized = Vec::with_capacity(rows.len());
    for (row, (name, codes)) in rows.iter().enumerate() {
        let invalid = |issue| AliasValidationError { row, issue };
        let name = name.trim();
        if name.chars().count() < 2
            || name.chars().all(|ch| ch.is_ascii_digit())
            || name.chars().any(|ch| ch.is_whitespace() || matches!(ch, ',' | '，' | '#' | ':'))
        {
            return Err(invalid(AliasIssue::Name));
        }
        let codes = alias_code_list(codes);
        if codes.is_empty() {
            return Err(invalid(AliasIssue::Items));
        }
        let mut canonical = Vec::with_capacity(codes.len());
        for token in codes {
            let code = token.strip_prefix('#').unwrap_or(&token);
            if !code.chars().all(|ch| ch.is_ascii_digit()) {
                return Err(invalid(AliasIssue::Code(token)));
            }
            let Some(id) = code.parse::<i64>().ok().filter(|id| *id > 0) else {
                return Err(invalid(AliasIssue::Code(token)));
            };
            let code = id.to_string();
            if !known_item(&code) {
                return Err(invalid(AliasIssue::Unknown(code)));
            }
            if !canonical.contains(&code) {
                canonical.push(code);
            }
        }
        normalized.push((name.to_string(), canonical.join(" ")));
    }
    Ok(alias_text(&normalized))
}

pub(crate) fn alias_code_list(codes: &str) -> Vec<String> {
    codes
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn alias_suggestions(catalog: &db::Catalog, query: &str, lang: &str, skip: &[String]) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    catalog
        .search("item", query, lang, "", "")
        .into_iter()
        .filter(|hit| hit.key.starts_with("item:"))
        .filter(|hit| {
            let code = hit.key.trim_start_matches("item:");
            !skip.iter().any(|item| item == code)
        })
        .take(8)
        .map(|hit| {
            let code = hit.key.trim_start_matches("item:");
            let (image, has_icon) = alias_image(&hit.icon);
            Hit {
                title: hit.title.into(),
                alternate_name: Default::default(),
                subtitle: format!("#{code}").into(),
                detail: String::new().into(),
                monster_drops: Default::default(),
                enchant_equipment: Default::default(),
                enchant_materials: Default::default(),
                enchant_scroll: false,
                enchant_level: Default::default(),
                enchant_rate: 0,
                enchant_bonuses: Default::default(),
                enchant_bonuses_recorded: false,
                spawn_places: Default::default(),
                stat_kind: String::new().into(),
                stat_element: String::new().into(),
                stat_hp: String::new().into(),
                stat_mp: String::new().into(),
                stat_hit: String::new().into(),
                stat_magic: String::new().into(),
                stat_atk: String::new().into(),
                stat_exp: String::new().into(),
                drop1: Image::default(),
                drop2: Image::default(),
                drop3: Image::default(),
                drop4: Image::default(),
                drop5: Image::default(),
                drop6: Image::default(),
                drop7: Image::default(),
                drop8: Image::default(),
                drop9: Image::default(),
                drop10: Image::default(),
                drop_total: 0,
                battle: Image::default(),
                has_battle: false,
                key: hit.key.into(),
                icon: image,
                has_icon,
                no_image: !has_icon,
                can_favourite: false,
                favourite: false,
                solo: false,
                has_badge: false,
                badge: Image::default(),
                aside: String::new().into(),
                chip: String::new().into(),
            }
        })
        .collect()
}

pub(crate) fn refresh_alias_icons(app: &App, catalog: &db::Catalog, rows: &[(String, String)]) {
    let lang = app.get_lang().to_string();
    let mut icons = Vec::new();
    for (index, (_, codes)) in rows.iter().enumerate() {
        for item in catalog.alias_items(codes, &lang) {
            let (image, has_icon) = alias_image(&item.icon);
            icons.push(Hit {
                title: item.title.clone().into(),
                alternate_name: Default::default(),
                subtitle: item.subtitle.clone().into(),
                detail: String::new().into(),
                monster_drops: Default::default(),
                enchant_equipment: Default::default(),
                enchant_materials: Default::default(),
                enchant_scroll: false,
                enchant_level: Default::default(),
                enchant_rate: 0,
                enchant_bonuses: Default::default(),
                enchant_bonuses_recorded: false,
                spawn_places: Default::default(),
                stat_kind: String::new().into(),
                stat_element: String::new().into(),
                stat_hp: String::new().into(),
                stat_mp: String::new().into(),
                stat_hit: String::new().into(),
                stat_magic: String::new().into(),
                stat_atk: String::new().into(),
                stat_exp: String::new().into(),
                drop1: Image::default(),
                drop2: Image::default(),
                drop3: Image::default(),
                drop4: Image::default(),
                drop5: Image::default(),
                drop6: Image::default(),
                drop7: Image::default(),
                drop8: Image::default(),
                drop9: Image::default(),
                drop10: Image::default(),
                drop_total: 0,
                battle: Image::default(),
                has_battle: false,
                key: item.key.clone().into(),
                icon: image,
                has_icon,
                no_image: !has_icon,
                can_favourite: false,
                favourite: false,
                solo: false,
                has_badge: false,
                badge: Image::default(),
                aside: index.to_string().into(),
                chip: String::new().into(),
            });
        }
    }
    app.set_alias_icons(ModelRc::new(VecModel::from(icons)));
}

pub(crate) fn alias_image(icon: &Option<(std::path::PathBuf, i32)>) -> (Image, bool) {
    if let Some((path, frame)) = icon {
        if let Ok(decoded) = agf::frame_rgba(path, *frame) {
            return (image_from_icon(decoded), true);
        }
    }
    (Image::default(), false)
}

pub(crate) fn show_alias_rows(app: &App, catalog: &db::Catalog, rows: &[(String, String)]) {
    let lang = app.get_lang().to_string();
    let mut icons = Vec::new();
    let shown: Vec<AliasRow> = rows
        .iter()
        .enumerate()
        .map(|(index, (name, codes))| {
            let items = catalog.alias_items(codes, &lang);
            for item in &items {
                let (image, has_icon) = alias_image(&item.icon);
                icons.push(Hit {
                    title: item.title.clone().into(),
                    alternate_name: Default::default(),
                    subtitle: item.subtitle.clone().into(),
                    detail: String::new().into(),
                    monster_drops: Default::default(),
                    enchant_equipment: Default::default(),
                    enchant_materials: Default::default(),
                    enchant_scroll: false,
                    enchant_level: Default::default(),
                    enchant_rate: 0,
                    enchant_bonuses: Default::default(),
                    enchant_bonuses_recorded: false,
                    spawn_places: Default::default(),
                    stat_kind: String::new().into(),
                    stat_element: String::new().into(),
                    stat_hp: String::new().into(),
                    stat_mp: String::new().into(),
                    stat_hit: String::new().into(),
                    stat_magic: String::new().into(),
                    stat_atk: String::new().into(),
                    stat_exp: String::new().into(),
                    drop1: Image::default(),
                    drop2: Image::default(),
                    drop3: Image::default(),
                    drop4: Image::default(),
                    drop5: Image::default(),
                    drop6: Image::default(),
                    drop7: Image::default(),
                    drop8: Image::default(),
                    drop9: Image::default(),
                    drop10: Image::default(),
                    drop_total: 0,
                    battle: Image::default(),
                    has_battle: false,
                    key: item.key.clone().into(),
                    icon: image,
                    has_icon,
                    no_image: !has_icon,
                    can_favourite: false,
                    favourite: false,
                    solo: false,
                    has_badge: false,
                    badge: Image::default(),
                    aside: index.to_string().into(),
                    chip: String::new().into(),
                });
            }
            AliasRow {
                name: name.into(),
                codes: codes.into(),
                detail: catalog.alias_names(codes, &lang).into(),
                count: items.len() as i32,
            }
        })
        .collect();
    app.set_alias_rows(ModelRc::new(VecModel::from(shown)));
    app.set_alias_icons(ModelRc::new(VecModel::from(icons)));
}

pub(crate) fn alias_text(rows: &[(String, String)]) -> String {
    rows.iter()
        .filter_map(|(name, codes)| {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let numbers: Vec<&str> = codes
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .collect();
            if numbers.is_empty() {
                return None;
            }
            Some(format!("{name} {}", numbers.join(" ")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
