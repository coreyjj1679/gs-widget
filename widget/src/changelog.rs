//! The same bilingual source supplies GitHub release notes and the offline UI.
use serde::Deserialize;
use slint::{ModelRc, VecModel};

use crate::{App, ChangelogItem, ChangelogRelease, ChangelogSection};

const BUNDLED_NOTES: &str = include_str!("../../release-notes.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notes {
    schema_version: u32,
    releases: Vec<Release>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    date: String,
    sections: Vec<Section>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    kind: Kind,
    en: Vec<String>,
    #[serde(rename = "zh-TW")]
    zh: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Added,
    Improved,
    Fixed,
}

impl Kind {
    fn headings(&self) -> (&'static str, &'static str) {
        match self {
            Self::Added => ("Added", "新增"),
            Self::Improved => ("Improvements", "改進"),
            Self::Fixed => ("Fixes", "修正"),
        }
    }
}

fn releases_for_version(source: &str, current: &str) -> Result<Vec<Release>, String> {
    let notes: Notes = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if notes.schema_version != 1 {
        return Err("Unsupported changelog schema".into());
    }
    let current = semver::Version::parse(current).map_err(|error| error.to_string())?;
    let mut releases = Vec::new();
    for release in notes.releases {
        let version =
            semver::Version::parse(&release.version).map_err(|error| error.to_string())?;
        if !version.pre.is_empty() || !version.build.is_empty() {
            return Err("Changelog versions must be stable".into());
        }
        if release.sections.is_empty()
            || release.sections.iter().any(|section| {
                section.en.is_empty()
                    || section.en.len() != section.zh.len()
                    || section
                        .en
                        .iter()
                        .chain(&section.zh)
                        .any(|text| text.trim().is_empty())
            })
        {
            return Err("Each changelog item needs both English and Traditional Chinese".into());
        }
        // A development checkout may contain notes for the next release. Never
        // present those as installed history in an older app.
        if version <= current {
            releases.push((version, release));
        }
    }
    releases.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(releases.into_iter().map(|(_, release)| release).collect())
}

pub fn install(app: &App) {
    let releases = match releases_for_version(BUNDLED_NOTES, env!("CARGO_PKG_VERSION")) {
        Ok(releases) => releases,
        Err(error) => {
            // Notes are nonessential: a malformed development file must never
            // prevent browsing the local catalogue or using personal settings.
            eprintln!("Could not load bundled changelog: {error}");
            Vec::new()
        }
    };
    let rows = releases
        .into_iter()
        .map(|release| ChangelogRelease {
            version: release.version.into(),
            date: release.date.into(),
            sections: ModelRc::new(VecModel::from(
                release
                    .sections
                    .into_iter()
                    .map(|section| {
                        let (en, zh) = section.kind.headings();
                        ChangelogSection {
                            heading_en: en.into(),
                            heading_zh: zh.into(),
                            entries: ModelRc::new(VecModel::from(
                                section
                                    .en
                                    .into_iter()
                                    .zip(section.zh)
                                    .map(|(en, zh)| ChangelogItem {
                                        text_en: en.into(),
                                        text_zh: zh.into(),
                                    })
                                    .collect::<Vec<_>>(),
                            )),
                        }
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect::<Vec<_>>();
    app.set_changelog_releases(ModelRc::new(VecModel::from(rows)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_history_is_bilingual_and_contains_the_running_version() {
        let releases = releases_for_version(BUNDLED_NOTES, env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(releases.first().unwrap().version, env!("CARGO_PKG_VERSION"));
        assert!(releases
            .iter()
            .flat_map(|release| &release.sections)
            .all(|section| { section.en.iter().zip(&section.zh).all(|(en, zh)| en != zh) }));
    }

    #[test]
    fn older_app_excludes_future_notes_and_orders_versions_numerically() {
        let source = r#"{"schema_version":1,"releases":[
            {"version":"1.9.0","date":"2026-10-18","sections":[{"kind":"fixed","en":["Fix"],"zh-TW":["修正"]}]},
            {"version":"2.0.0","date":"2026-10-18","sections":[{"kind":"added","en":["New"],"zh-TW":["新增"]}]},
            {"version":"1.10.0","date":"2026-10-18","sections":[{"kind":"improved","en":["Better"],"zh-TW":["改進"]}]}
        ]}"#;
        let releases = releases_for_version(source, "1.10.0").unwrap();
        assert_eq!(
            releases
                .iter()
                .map(|release| release.version.as_str())
                .collect::<Vec<_>>(),
            ["1.10.0", "1.9.0"]
        );
    }

    #[test]
    fn unsupported_schema_or_missing_translation_is_not_displayed() {
        assert!(releases_for_version(r#"{"schema_version":2,"releases":[]}"#, "1.0.1").is_err());
        let source = r#"{"schema_version":1,"releases":[{"version":"1.0.1","date":"2026-10-18","sections":[{"kind":"fixed","en":["Fix"],"zh-TW":[]}]}]}"#;
        assert!(releases_for_version(source, "1.0.1").is_err());
    }
}
