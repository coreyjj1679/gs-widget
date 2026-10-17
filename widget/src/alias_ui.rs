use slint::{Image, ModelRc, VecModel};

use super::db;
use super::agf;
use super::icons::image_from_icon;
use super::{AliasRow, App, Hit};

#[cfg(test)]
mod tests {
    use super::{alias_code_list, alias_text};

    #[test]
    fn alias_codes_split_on_commas_and_blank_rows_drop_out() {
        assert_eq!(alias_code_list("6213, 6214，6215"), vec!["6213", "6214", "6215"]);
        assert_eq!(
            alias_text(&[(" SSA ".into(), "6213 6214".into()), (String::new(), "1".into())]),
            "SSA 6213 6214"
        );
    }
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
                subtitle: format!("#{code}").into(),
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
                subtitle: item.subtitle.clone().into(),
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
                    subtitle: item.subtitle.clone().into(),
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
