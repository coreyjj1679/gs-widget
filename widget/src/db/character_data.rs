use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use super::hero::{hero_land_pair, hero_names};
use super::rows::{CompanionRow, HeroRow, RecruitmentMethod, RecruitmentMethods};
use super::store::{column_exists, table_exists};
use super::text::{caption, chrome_text, commas, element_name, lead, lead3, phrase, trail3};
use super::{Catalog, Hit};

pub(in crate::db) const RECRUITMENT_METHODS: [&str; 4] = [
    "recruitment:trainer", "recruitment:craft_certificate", "recruitment:certificate",
    "recruitment:certificate_bound",
];

pub(in crate::db) fn player_nationality_name(code: i64, lang: &str) -> Option<String> {
    let (en, zh) = match code {
        0 => ("Any nationality", "不限"),
        1 | 10 | 100 | 1000 | 10000 => hero_land_pair(code),
        _ => return None,
    };
    Some(chrome_text(lang, zh, en))
}

pub(in crate::db) const INDIA_OR_SECOND_PROMOTION: &str = "india_or_second_promotion";

pub(in crate::db) fn load_promotion_nationalities(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    if !table_exists(conn, "character_facts") {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare(
        "SELECT code, value_en FROM character_facts WHERE label_en = 'Promotion nationality'",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

impl HeroRow {
    pub(in crate::db) fn promotion_nationality_fact(&self, lang: &str) -> Option<(String, String)> {
        let rule = self.promotion_nationality.as_deref()?;
        let value = if rule == INDIA_OR_SECOND_PROMOTION {
            chrome_text(lang, "印度籍，或任何國籍且主角已二轉", "India, or any nationality after the main character's second promotion")
        } else {
            player_nationality_name(rule.parse().ok()?, lang)?
        };
        let label = chrome_text(lang, "轉職所需主角國籍", "Main character nationality for promotion");
        Some((label, value))
    }

    pub(in crate::db) fn promotion_condition(&self, lang: &str) -> Option<String> {
        self.promotion_nationality_fact(lang).map(|(label, value)| format!("{label} · {value}"))
    }
}

impl RecruitmentMethod {
    pub(in crate::db) fn eligibility(&self, lang: &str) -> Vec<String> {
        let mut terms = Vec::new();
        if let Some(name) = self.nationality.and_then(|code| player_nationality_name(code, lang)) {
            let label = chrome_text(lang, "所需主角國籍：", "Required player nationality: ");
            terms.push(format!("{label}{name}"));
        }
        for (en, zh) in &self.credit_levels {
            let level = lead(lang, en, zh);
            if !level.is_empty() {
                terms.push(format!("{} {level}", chrome_text(lang, "所需信用等級", "Required credit level")));
            }
        }
        terms
    }
}

pub(in crate::db) fn load_recruitment_methods(
    conn: &Connection,
) -> rusqlite::Result<HashMap<String, RecruitmentMethods>> {
    let mut grouped = HashMap::<String, RecruitmentMethods>::new();
    if !table_exists(conn, "character_materials") {
        return Ok(grouped);
    }
    let mut stmt = conn.prepare(
        "SELECT code, kind, item_code, qty FROM character_materials
         WHERE kind IN ('recruitment:trainer', 'recruitment:certificate', 'recruitment:certificate_bound')
         ORDER BY code, kind, slot",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    for row in rows {
        let (code, method, item, quantity) = row?;
        grouped.entry(code).or_default()
            .entry(method).or_default()
            .materials.push((item, quantity));
    }
    load_certificate_crafting(conn, &mut grouped)?;
    if grouped.is_empty() || !table_exists(conn, "character_facts") {
        return Ok(grouped);
    }
    let mut stmt = conn.prepare(
        "SELECT code, label_en, value_en, value_zh FROM character_facts
         WHERE label_en IN ('Trainer nationality','Certificate nationality','Bound certificate nationality',
                            'Trainer credit level','Trainer fee')
         ORDER BY code, slot",
    )?;
    let rows = stmt.query_map([], |row| Ok((
        row.get::<_, String>(0)?, row.get::<_, String>(1)?,
        row.get::<_, String>(2)?, row.get::<_, String>(3)?,
    )))?;
    for row in rows {
        let (code, label, en, zh) = row?;
        let method_key = match label.as_str() {
            "Certificate nationality" => "recruitment:certificate",
            "Bound certificate nationality" => "recruitment:certificate_bound",
            _ => "recruitment:trainer",
        };
        let Some(method) = grouped.get_mut(&code).and_then(|methods| methods.get_mut(method_key)) else { continue };
        match label.as_str() {
            "Trainer nationality" | "Certificate nationality" | "Bound certificate nationality" => {
                method.nationality = lead("en", &en, &zh).parse().ok()
                    .filter(|code| player_nationality_name(*code, "en").is_some());
            }
            "Trainer credit level" => method.credit_levels.push((en, zh)),
            "Trainer fee" => method.fee = lead("en", &en, &zh).parse().ok().filter(|fee| *fee >= 0),
            _ => {}
        }
    }
    Ok(grouped)
}

fn load_certificate_crafting(conn: &Connection, methods: &mut HashMap<String, RecruitmentMethods>) -> rusqlite::Result<()> {
    if methods.is_empty() || !column_exists(conn, "recipes", "npc") || !table_exists(conn, "recipe_materials") {
        return Ok(());
    }
    // Join on the actual certificate consumed. In particular, Zhang Guolao
    // makes the bound sorcerer certificates, which have distinct item codes.
    let mut stmt = conn.prepare(
        "WITH crafts AS (
            SELECT c.code, r.id, r.result_item, r.fee
            FROM character_materials c JOIN recipes r ON r.result_item = c.item_code
            WHERE c.kind IN ('recruitment:certificate', 'recruitment:certificate_bound')
              AND c.qty = 1 AND r.result_qty = 1 AND r.fee >= 0
              AND r.source = 'unified' AND r.npc = '1012'
              AND NOT EXISTS (SELECT 1 FROM recipe_materials m WHERE m.recipe_id = r.id
                              AND (m.qty <= 0 OR m.enhance != 0))
         )
         SELECT c.code, c.id, c.result_item, c.fee, m.item_code, m.qty FROM crafts c
         JOIN recipe_materials m ON m.recipe_id = c.id
         WHERE c.code IN (SELECT code FROM crafts GROUP BY code HAVING count(*) = 1)
         ORDER BY c.code, m.slot",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let code: String = row.get(0)?;
        let Some(routes) = methods.get_mut(&code) else { continue };
        let craft = routes.entry("recruitment:craft_certificate".into()).or_default();
        craft.certificate_recipe = Some((row.get(1)?, row.get(2)?));
        craft.fee = Some(row.get(3)?);
        craft.materials.push((row.get(4)?, row.get(5)?));
    }
    Ok(())
}

pub(in crate::db) fn load_companions(conn: &Connection) -> HashMap<String, CompanionRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT code, riding_level, movement_bonus, movement_scope,
                family_element, spirit_grade, evolution_root, evolution_order FROM character_companions",
    ) else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            CompanionRow {
                riding_level: row.get(1)?,
                movement_bonus: row.get(2)?,
                movement_scope: row.get(3)?,
                family_element: row.get(4)?,
                spirit_grade: row.get(5)?,
                evolution_root: row.get(6)?,
                evolution_order: row.get(7)?,
            },
        ))
    }) else {
        return HashMap::new();
    };
    rows.flatten().collect()
}

pub(in crate::db) fn spirit_grade(grade: i64, lang: &str) -> String {
    match grade {
        1 => chrome_text(lang, "下級", "Beginner"),
        2 => chrome_text(lang, "中級", "Intermediate"),
        3 => chrome_text(lang, "上級", "Advanced"),
        4 => chrome_text(lang, "最高級", "Highest"),
        _ => chrome_text(lang, &format!("{grade} 階"), &format!("Grade {grade}")),
    }
}

pub(in crate::db) fn spirit_family_name(element: i64, lang: &str) -> String {
    let (zh, en) = match element {
        0 => ("無屬性", "Non-elemental"),
        1 => ("火", "Fire"),
        2 => ("水", "Water"),
        3 => ("風", "Wind"),
        4 => ("雷", "Thunder"),
        5 => ("地", "Earth"),
        _ => ("屬性未記錄", "Element not recorded"),
    };
    chrome_text(lang, zh, en)
}

pub(in crate::db) fn companion_summary(companion: &CompanionRow, rank: i64, lang: &str) -> String {
    let mut parts = Vec::new();
    if rank == 13 {
        if let Some(level) = companion.riding_level {
            parts.push(chrome_text(
                lang,
                &format!("馬術 {level}"),
                &format!("Riding {level}"),
            ));
        } else {
            parts.push(chrome_text(lang, "馬術未記錄", "Riding unknown"));
        }
        if let Some(bonus) = companion.movement_bonus {
            let scope = match companion.movement_scope {
                Some(0) => chrome_text(lang, "主角", "Main character"),
                Some(1) => chrome_text(lang, "全隊", "Party"),
                _ => chrome_text(lang, "對象未記錄", "Scope not recorded"),
            };
            parts.push(chrome_text(
                lang,
                &format!("{scope}移速 {bonus:+}"),
                &format!("{scope} speed {bonus:+}"),
            ));
        } else {
            parts.push(chrome_text(
                lang,
                "移速加成未記錄",
                "Movement bonus unknown",
            ));
        }
    } else if rank == 14 {
        if let Some(element) = companion.family_element {
            parts.push(spirit_family_name(element, lang));
        } else {
            parts.push(chrome_text(lang, "屬性未記錄", "Element not recorded"));
        }
        if let Some(grade) = companion.spirit_grade {
            parts.push(spirit_grade(grade, lang));
        } else {
            parts.push(chrome_text(lang, "階級未記錄", "Grade not recorded"));
        }
    }
    parts.join(" · ")
}

// Character enrichment is optional so catalogs made before this import remain readable.
fn stat_lines(conn: &Connection, code: &str, lang: &str) -> Vec<(String, String)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT strength, dexterity, vitality, intelligence, hp, mp, hit_resist, magic_resist,
                atk_min, atk_max, move_speed, attack_speed, armor, unit_type, element, element_value
         FROM character_stats WHERE code = ?1",
    ) else {
        return Vec::new();
    };
    let Ok(values) = stmt.query_row([code], |row| {
        (0..16)
            .map(|index| row.get::<_, Option<i64>>(index))
            .collect::<rusqlite::Result<Vec<_>>>()
    }) else {
        return Vec::new();
    };
    let labels = [
        ("力量", "STR"),
        ("敏捷", "DEX"),
        ("生命力", "VIT"),
        ("智力", "INT"),
        ("體力", "HP"),
        ("魔力", "MP"),
        ("打擊抗性", "Hit resist"),
        ("魔法抗性", "Magic resist"),
    ];
    let mut lines = Vec::new();
    for (index, (zh, en)) in labels.into_iter().enumerate() {
        if let Some(value) = values[index] {
            lines.push((chrome_text(lang, zh, en), commas(value)));
        }
    }
    match (values[8], values[9]) {
        (Some(min), Some(max)) => lines.push((
            phrase("atk", lang),
            format!("{}–{}", commas(min), commas(max)),
        )),
        (Some(min), None) => lines.push((chrome_text(lang, "最低攻擊", "ATK min"), commas(min))),
        (None, Some(max)) => lines.push((chrome_text(lang, "最高攻擊", "ATK max"), commas(max))),
        (None, None) => {}
    }
    for (index, zh, en) in [
        (10, "移動速度", "Move speed"),
        (11, "攻擊速度", "Attack speed"),
        (12, "防禦力", "Armor"),
    ] {
        if let Some(value) = values[index] {
            lines.push((chrome_text(lang, zh, en), commas(value)));
        }
    }
    if let Some(unit) = values[13] {
        let value = match unit {
            0 => phrase("ground", lang),
            1 => phrase("air", lang),
            _ => unit.to_string(),
        };
        lines.push((phrase("unit", lang), value));
    }
    if let Some(element) = values[14] {
        lines.push((phrase("element", lang), element_name(element, lang)));
    }
    if let Some(value) = values[15] {
        lines.push((phrase("ela", lang), commas(value)));
    }
    lines
}

fn extra_facts(conn: &Connection, code: &str, lang: &str, stats: bool, omitted: &[&str]) -> Vec<(String, String)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT slot, label_en, label_zh, value_en, value_zh FROM character_facts WHERE code = ?1 ORDER BY slot",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([code], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten()
        // Slots below 100 are source-defined mount bonuses, rather than unit base stats.
        .filter(|(slot, en, ..)| {
            (*slot < 100) == stats
                && en != "Skill effect" && en != "Promotion nationality"
                && !omitted.contains(&en.as_str())
        })
        .map(|(_, en, zh, value_en, value_zh)| {
            let value = lead(lang, &value_en, &value_zh);
            let value = match en.as_str() {
                "Trainer nationality" | "Certificate nationality" | "Bound certificate nationality" => value.parse().ok()
                    .and_then(|code| player_nationality_name(code, lang)).unwrap_or_default(),
                "Trainer fee" => value.parse().map(commas).unwrap_or(value),
                _ => value,
            };
            (chrome_text(lang, &zh, &en), value)
        })
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

fn material_rows(conn: &Connection, code: &str) -> Vec<(String, i64, i64)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT kind, item_code, qty FROM character_materials WHERE code = ?1 ORDER BY kind, slot",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([code], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

struct IncomingSpiritEvolution {
    source_code: String,
    intimacy: Option<String>,
    materials: Vec<(i64, i64)>,
}

fn incoming_spirit_evolutions(conn: &Connection, code: &str, lang: &str) -> Vec<IncomingSpiritEvolution> {
    // PetInfo stores the recipe on the source form. Present it on the form the
    // user wants to obtain, without reversing ordinary character/job or mount
    // requirements. Joins guard both ends even in partially enriched catalogs.
    let Ok(mut stmt) = conn.prepare(
        "SELECT upgrade.code FROM character_upgrades AS upgrade
         JOIN characters AS source ON source.code = upgrade.code AND source.rank = 14
         JOIN characters AS target ON target.code = upgrade.target_code AND target.rank = 14
         WHERE upgrade.target_code = ?1 ORDER BY upgrade.code",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([code], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.flatten()
        .map(|source_code| {
            let intimacy = conn.query_row(
                "SELECT value_en, value_zh FROM character_facts
                 WHERE code = ?1 AND label_en = 'Evolution intimacy' ORDER BY slot LIMIT 1",
                [&source_code],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            ).ok()
                .map(|(en, zh)| lead(lang, &en, &zh))
                .filter(|value| !value.is_empty());
            let materials = material_rows(conn, &source_code)
                .into_iter()
                .filter_map(|(kind, item, qty)| (kind == "evolution").then_some((item, qty)))
                .collect();
            IncomingSpiritEvolution { source_code, intimacy, materials }
        })
        .collect()
}

pub(in crate::db) fn material_heading(kind: &str, lang: &str) -> String {
    match kind {
        "recruitment:trainer" => chrome_text(lang, "訓練所招募", "Recruit at the monster trainer"),
        "recruitment:certificate" => chrome_text(lang, "使用證書", "Use a certificate"),
        "recruitment:certificate_bound" => chrome_text(lang, "使用不可交易證書", "Use an untradeable certificate"),
        "recruitment:craft_certificate" => chrome_text(lang, "委託神仙谷的張果老製作證書", "Craft the certificate with Zhang Guolao in Taoist Hermit Valley"),
        "evolution" => chrome_text(lang, "升級材料", "Upgrade materials"),
        "acquisition" => chrome_text(lang, "獲得材料", "Acquisition materials"),
        _ => kind
            .strip_prefix("riding:")
            .map(|level| {
                chrome_text(
                    lang,
                    &format!("馬術 {level} 級材料"),
                    &format!("Riding level {level} materials"),
                )
            })
            .unwrap_or_else(|| phrase("craft-need", lang)),
    }
}

fn section(key: String, title: String) -> Hit {
    Hit {
        title,
        subtitle: String::new(),
        key,
        icon: None,
        solo: false,
        badge: None,
        aside: String::new(),
        chip: "section".into(),
    }
}

impl Catalog {
    pub fn hero_effect_lines(&self, code: &str, lang: &str) -> Vec<(String, String)> {
        self.hero_effect_texts(code, lang, false)
    }

    pub fn hero_secondary_effect_lines(&self, code: &str, lang: &str) -> Vec<(String, String)> {
        if super::secondary_lang(lang).is_none() { return Vec::new(); }
        self.hero_effect_texts(code, lang, true)
    }

    fn hero_effect_texts(&self, code: &str, lang: &str, secondary: bool) -> Vec<(String, String)> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT label_en, label_zh, value_en, value_zh FROM character_facts
             WHERE code = ?1 AND label_en = 'Skill effect' ORDER BY slot",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([code], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        }) else {
            return Vec::new();
        };
        rows.flatten()
            .map(|(en, zh, value_en, value_zh)| {
                (chrome_text(lang, &zh, &en), if secondary { trail3(lang, &value_en, &value_zh, "") } else { lead(lang, &value_en, &value_zh) })
            })
            .filter(|(_, value)| !value.is_empty())
            .collect()
    }

    pub fn hero_stat_lines(&self, code: &str, lang: &str) -> Vec<(String, String)> {
        let mut lines = stat_lines(&self.conn, code, lang);
        lines.extend(extra_facts(&self.conn, code, lang, true, &[]));
        lines
    }

    pub(in crate::db) fn hero_data_facts(&self, code: &str, lang: &str, inline_methods: Option<&RecruitmentMethods>) -> Vec<(String, String)> {
        let mut omitted = Vec::new();
        if self.hero_is_spirit(code) {
            omitted.push("Evolution intimacy");
        }
        if let Some(methods) = inline_methods {
            if methods.contains_key("recruitment:trainer") {
                omitted.extend(["Trainer nationality", "Trainer credit level", "Trainer fee"]);
            }
            if methods.contains_key("recruitment:certificate") {
                omitted.push("Certificate nationality");
            }
            if methods.contains_key("recruitment:certificate_bound") {
                omitted.push("Bound certificate nationality");
            }
        }
        extra_facts(&self.conn, code, lang, false, &omitted)
    }

    pub(in crate::db) fn hero_data_links(&self, code: &str, lang: &str) -> Vec<Hit> {
        let is_spirit = self.hero_is_spirit(code);
        let mut hits = Vec::new();
        if let Some(hero) = self.index.heroes.iter()
            .find(|hero| hero.code == code && !hero.recruitment_methods.is_empty())
        {
            let heading = if hero.recruitment_methods.len() > 1 {
                chrome_text(lang, "招募材料（任選一種方式）", "Recruitment materials (choose one method)")
            } else {
                chrome_text(lang, "招募材料", "Recruitment materials")
            };
            hits.push(section("recruitment".into(), heading));
            for method in RECRUITMENT_METHODS {
                if let Some(data) = hero.recruitment_methods.get(method) {
                    let mut terms = data.eligibility(lang);
                    if let Some((_, certificate)) = data.certificate_recipe {
                        terms.push(format!("{} ×1", self.item_name(certificate, lang)));
                        if let Some(fee) = data.fee {
                            terms.push(format!("{} {} {}", chrome_text(lang, "製作費", "Crafting fee"),
                                commas(fee), chrome_text(lang, "兩", "coins")));
                        }
                    }
                    if method == "recruitment:trainer" {
                        terms.push(match data.fee {
                            Some(amount) => format!("{} {} {}", chrome_text(lang, "僱用費", "Hiring fee"),
                                commas(amount), chrome_text(lang, "兩", "coins")),
                            None => chrome_text(lang, "僱用費未記錄", "Hiring fee not recorded"),
                        });
                    }
                    self.index.push_material_block(
                        &mut hits, lang, material_heading(method, lang), terms.join(" · "), &data.materials,
                    );
                }
            }
        }
        if is_spirit {
            for evolution in incoming_spirit_evolutions(&self.conn, code, lang) {
                let mut requirements = Vec::new();
                if let Some(hit) = self.companion_link(
                    &evolution.source_code, lang, chrome_text(lang, "前置精靈", "Required spirit"),
                ) {
                    requirements.push(hit);
                }
                if let Some(intimacy) = evolution.intimacy {
                    requirements.push(Hit {
                        title: chrome_text(lang, "所需親密度", "Required intimacy"),
                        subtitle: String::new(),
                        key: String::new(),
                        icon: None,
                        solo: false,
                        badge: None,
                        aside: intimacy,
                        chip: String::new(),
                    });
                }
                requirements.extend(evolution.materials.into_iter().map(|(item, qty)| {
                    self.companion_material_link(item, qty, lang)
                }));
                if !requirements.is_empty() {
                    hits.push(section(
                        format!("companion:incoming-evolution:{}", evolution.source_code),
                        chrome_text(lang, "升級至此精靈", "Upgrade to this spirit"),
                    ));
                    hits.extend(requirements);
                }
            }
        }
        let mut groups: BTreeMap<String, Vec<Hit>> = BTreeMap::new();
        for (kind, item_code, qty) in material_rows(&self.conn, code) {
            if (is_spirit && kind == "evolution") || RECRUITMENT_METHODS.contains(&kind.as_str()) {
                continue;
            }
            groups.entry(kind).or_default().push(self.companion_material_link(item_code, qty, lang));
        }
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT kind, target_code FROM character_requirements WHERE code = ?1 ORDER BY kind, target_code",
        ) {
            if let Ok(rows) = stmt.query_map([code], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))) {
                for (kind, target) in rows.flatten() {
                    if let Some(hit) = self.companion_link(&target, lang, chrome_text(lang, "所需坐騎", "Required mount")) {
                        groups.entry(kind).or_default().insert(0, hit);
                    }
                }
            }
        }
        for (kind, materials) in groups {
            if !materials.is_empty() {
                hits.push(section(
                    format!("companion:{kind}"),
                    material_heading(&kind, lang),
                ));
                hits.extend(materials);
            }
        }
        let mut upgrades = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT target_code, 1 FROM character_upgrades WHERE code = ?1
             UNION ALL SELECT code, 0 FROM character_upgrades WHERE target_code = ?1 ORDER BY 2, 1",
        ) {
            if let Ok(rows) = stmt.query_map([code], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            }) {
                for (target, next) in rows.flatten() {
                    // The previous spirit already appears as the required
                    // source above. Retain the outgoing navigation separately.
                    if is_spirit && !next {
                        continue;
                    }
                    let label = if next {
                        chrome_text(lang, "下一階段", "Next form")
                    } else {
                        chrome_text(lang, "上一階段", "Previous form")
                    };
                    if let Some(hit) = self.companion_link(&target, lang, label) {
                        upgrades.push(hit);
                    }
                }
            }
        }
        if !upgrades.is_empty() {
            hits.push(section(
                "companion:upgrades".into(),
                chrome_text(lang, "升級路線", "Upgrade path"),
            ));
            hits.extend(upgrades);
        }
        hits
    }

    pub(in crate::db) fn recruitment_item_links(&self, item: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for hero in &self.index.heroes {
            for (method, data) in &hero.recruitment_methods {
                // These ingredients already have a link to the certificate's recipe under item uses.
                if data.certificate_recipe.is_some() { continue; }
                for (_, quantity) in data.materials.iter().filter(|(code, _)| *code == item) {
                    let (title, alternate) = hero_names(lang, hero);
                    hits.push(Hit {
                        title,
                        subtitle: caption(&alternate, material_heading(method, lang)),
                        key: format!("hero:{}", hero.code),
                        icon: hero.icon.clone(),
                        solo: false,
                        badge: None,
                        aside: format!("× {}", commas(*quantity)),
                        chip: String::new(),
                    });
                }
            }
        }
        hits
    }

    fn companion_material_link(&self, item_code: i64, qty: i64, lang: &str) -> Hit {
        let item = self.index.find_item(item_code);
        let (name, alt) = item.map(|item| (lead3(lang, &item.en, &item.zh, &item.ko), trail3(lang, &item.en, &item.zh, &item.ko)))
            .unwrap_or_default();
        Hit {
            title: if name.is_empty() { format!("#{item_code}") } else { name },
            subtitle: alt,
            key: if item.is_some() { format!("item:{item_code}") } else { String::new() },
            icon: self.index.item_icons.get(&item_code).cloned(),
            solo: false,
            badge: None,
            aside: format!("× {}", commas(qty)),
            chip: String::new(),
        }
    }

    fn companion_link(&self, code: &str, lang: &str, relation: String) -> Option<Hit> {
        let hero = self.index.heroes.iter().find(|hero| hero.code == code)?;
        let (title, alt) = hero_names(lang, hero);
        Some(Hit {
            title,
            subtitle: alt,
            key: format!("hero:{code}"),
            icon: hero.icon.clone(),
            solo: false,
            badge: None,
            aside: relation,
            chip: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_catalogs_do_not_invent_stats() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(stat_lines(&conn, "U00", "en").is_empty());
        assert!(extra_facts(&conn, "U00", "en", false, &[]).is_empty());
        assert!(incoming_spirit_evolutions(&conn, "U00", "en").is_empty());
        assert!(load_companions(&conn).is_empty());
        assert!(load_recruitment_methods(&conn).unwrap().is_empty());
    }

    #[test]
    fn nationality_is_scoped_to_the_recorded_method() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE character_materials (code TEXT, kind TEXT, item_code INTEGER, qty INTEGER, slot INTEGER);
             INSERT INTO character_materials VALUES ('test','recruitment:trainer',853,4,0), ('test','recruitment:certificate',897,1,0);
             CREATE TABLE character_facts (code TEXT, label_en TEXT, value_en TEXT, value_zh TEXT, slot INTEGER);
             INSERT INTO character_facts VALUES ('test','Trainer nationality','0','0',0), ('test','Trainer fee','35000','35000',1);",
        ).unwrap();
        let methods = load_recruitment_methods(&conn).unwrap();
        let trainer = &methods["test"]["recruitment:trainer"];
        let certificate = &methods["test"]["recruitment:certificate"];
        assert_eq!(trainer.nationality, Some(0));
        assert_eq!(trainer.eligibility("zh"), ["所需主角國籍：不限"]);
        assert_eq!(certificate.nationality, None);
        assert!(certificate.eligibility("zh").is_empty());
        assert_eq!(certificate.fee, None);
    }

    #[test]
    fn seven_npc_certificates_show_their_own_recipe_before_redemption() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, item, fee) in [
            ("H44", 6411, "5,000,000"), ("H45", 6412, "5,000,000"),
            ("H46", 6413, "5,000,000"), ("H47", 6414, "5,000,000"),
            ("F40", 6967, "5,000,000"), ("F41", 7587, "200,000,000"),
            ("F42", 7588, "200,000,000"),
        ] {
            let rows = catalog.hero_data_links(code, "zh-en");
            assert_eq!(rows[0].key, "recruitment");
            assert_eq!(rows[1].title, "委託神仙谷的張果老製作證書");
            assert!(rows[1].subtitle.contains(&catalog.item_name(item, "zh-en")));
            assert!(rows[1].subtitle.contains(&format!("製作費 {fee} 兩")));
            assert!(rows[2..7].iter().all(|row| row.key.starts_with("item:")));
            assert_eq!(rows[7].title, "使用證書");
            assert_eq!(rows[7].subtitle, "所需主角國籍：不限");
            assert!(rows.iter().any(|row| row.key == format!("item:{item}") && row.aside == "× 1"));
            if matches!(code, "F41" | "F42") {
                assert_eq!(rows[9].title, "使用不可交易證書");
                assert_eq!(rows[9].subtitle, "所需主角國籍：不限");
            }
        }
    }

    #[test]
    fn donkey_recruitment_methods_have_separate_linked_materials_and_reverse_uses() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let facts = catalog.hero_sheet("U85", "en").unwrap().3;
        assert!(facts.contains(&("Trainer fee".into(), "35,000".into())));
        assert!(facts.contains(&("Trainer credit level".into(), "35".into())));
        assert!(facts.contains(&("Trainer nationality".into(), "Any nationality".into())));
        let native_facts = catalog.hero_inline_sheet("U85", "en").unwrap().3;
        assert!(native_facts.iter().all(|(label, _)| !label.starts_with("Trainer ")));
        let (_, links, _) = catalog.open_key("hero:U85", "zh-en");
        let start = links.iter().position(|row| row.key == "recruitment").unwrap();
        assert_eq!(links[start].title, "招募材料（任選一種方式）");
        let routes: Vec<_> = links[start + 1..].iter().take_while(|row| row.chip != "section")
            .map(|row| (row.title.as_str(), row.key.as_str(), row.aside.as_str())).collect();
        assert_eq!(routes, [
            ("訓練所招募", "", ""), ("封印珠", "item:853", "× 4"),
            ("使用證書", "", ""), ("驢的證書", "item:897", "× 1"),
        ]);
        assert_eq!(links[start + 1].subtitle, "所需主角國籍：不限 · 所需信用等級 35 · 僱用費 35,000 兩");
        assert!(!links[start + 3].subtitle.contains("僱用費"));
        for (lang, trainer_terms) in [
            ("en", "Required player nationality: Any nationality · Required credit level 35 · Hiring fee 35,000 coins"),
            ("ko", "필요한 주인공 국적: 제한 없음 · 필요 신용등급 35 · 고용 비용 35,000 냥"),
        ] {
            let rows = catalog.hero_data_links("U85", lang);
            assert_eq!(rows[1].subtitle, trainer_terms);
        }
        let bandit = catalog.hero_data_links("M06", "ko");
        assert!(bandit[1].subtitle.contains("필요한 주인공 국적: 조선"));
        assert_eq!(bandit[3].subtitle, "필요한 주인공 국적: 조선");
        for (code, quantity, method) in [(853, "× 4", "訓練所招募"), (897, "× 1", "使用證書")] {
            let links = catalog.item_relations(code, "zh-en");
            let uses: Vec<_> = links.iter().filter(|row| row.key == "hero:U85").collect();
            assert_eq!(uses.len(), 1);
            assert_eq!(uses[0].aside, quantity);
            assert!(uses[0].subtitle.contains(method));
        }
    }

    #[test]
    fn missing_companion_metadata_is_not_shown_as_zero_bonus() {
        let mut companion = CompanionRow::default();
        let missing = companion_summary(&companion, 13, "en");
        assert!(missing.contains("Riding unknown") && missing.contains("Movement bonus unknown"));
        assert!(!missing.contains("+0"));
        companion.riding_level = Some(1);
        companion.movement_bonus = Some(0);
        companion.movement_scope = Some(0);
        assert_eq!(
            companion_summary(&companion, 13, "zh"),
            "馬術 1 · 主角移速 +0"
        );
    }

    #[test]
    fn unknown_stats_stay_absent_and_recorded_zero_is_preserved() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE character_stats (code TEXT, strength INTEGER, dexterity INTEGER, vitality INTEGER,
                intelligence INTEGER, hp INTEGER, mp INTEGER, hit_resist INTEGER, magic_resist INTEGER,
                atk_min INTEGER, atk_max INTEGER, move_speed INTEGER, attack_speed INTEGER, armor INTEGER,
                unit_type INTEGER, element INTEGER, element_value INTEGER);
             INSERT INTO character_stats (code, strength, mp, atk_max) VALUES ('test', 15, 0, 90);",
        ).unwrap();
        let lines = stat_lines(&conn, "test", "en");
        assert_eq!(
            lines,
            vec![
                ("STR".into(), "15".into()),
                ("MP".into(), "0".into()),
                ("ATK max".into(), "90".into())
            ]
        );
        assert!(stat_lines(&conn, "missing", "en").is_empty());
    }

    #[test]
    fn mount_bonuses_and_requirements_are_separate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE character_facts (code TEXT, slot INTEGER, label_en TEXT, label_zh TEXT, value_en TEXT, value_zh TEXT);
             INSERT INTO character_facts VALUES ('mount:1', 1, 'Speed bonus', '移速加成', '+10%', '+10%');
             INSERT INTO character_facts VALUES ('mount:1', 100, 'Riding', '馬術', 'Level 2', '2 級');",
        ).unwrap();
        assert_eq!(
            extra_facts(&conn, "mount:1", "en", true, &[]),
            vec![("Speed bonus".into(), "+10%".into())]
        );
        assert_eq!(
            extra_facts(&conn, "mount:1", "zh", false, &[]),
            vec![("馬術".into(), "2 級".into())]
        );
    }

    #[test]
    fn incoming_requirements_reverse_only_spirit_evolution_and_keep_unknown_intimacy_absent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE characters (code TEXT PRIMARY KEY, rank INTEGER);
             CREATE TABLE character_upgrades (code TEXT, target_code TEXT);
             CREATE TABLE character_materials (code TEXT, kind TEXT, slot INTEGER, item_code INTEGER, qty INTEGER);
             CREATE TABLE character_facts (code TEXT, slot INTEGER, label_en TEXT, value_en TEXT, value_zh TEXT);
             INSERT INTO characters VALUES ('source',14),('target',14),('hero',0),('mount',13);
             INSERT INTO character_upgrades VALUES ('source','target'),('hero','target'),('source','hero'),('source','mount');
             INSERT INTO character_materials VALUES ('source','evolution',1,100,30),('source','acquisition',2,200,4);",
        ).unwrap();
        let incoming = incoming_spirit_evolutions(&conn, "target", "en");
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source_code, "source");
        assert_eq!(incoming[0].materials, vec![(100, 30)]);
        assert!(incoming[0].intimacy.is_none());
        assert!(incoming_spirit_evolutions(&conn, "source", "en").is_empty());
        assert!(incoming_spirit_evolutions(&conn, "hero", "en").is_empty());
        assert!(incoming_spirit_evolutions(&conn, "mount", "en").is_empty());
    }

    #[test]
    fn fifth_grade_spirit_shows_the_fourth_to_fifth_recipe_on_its_own_page() {
        let catalog = Catalog::open().expect("catalog");
        let links = catalog.hero_data_links("V02", "en");
        assert_eq!(links[0].title, "Upgrade to this spirit");
        assert!(links.iter().any(|hit| hit.key == "hero:H30" && hit.aside == "Required spirit"));
        assert!(links.iter().any(|hit| {
            hit.title == "Required intimacy" && hit.aside == "50,000" && hit.key.is_empty()
        }));
        let materials: Vec<_> = links.iter()
            .filter(|hit| hit.key.starts_with("item:"))
            .map(|hit| (hit.key.as_str(), hit.aside.as_str()))
            .collect();
        assert_eq!(materials, vec![
            ("item:8628", "× 1"),
            ("item:9744", "× 30"),
            ("item:3506", "× 3"),
            ("item:5250", "× 1"),
        ]);
        assert!(!links.iter().any(|hit| hit.aside == "Next form"));
        let zh = catalog.hero_data_links("V02", "zh");
        assert_eq!(zh[0].title, "升級至此精靈");
        assert!(zh.iter().any(|hit| hit.title == "所需親密度" && hit.aside == "50,000"));
    }

    #[test]
    fn middle_spirit_shows_incoming_recipe_and_only_a_link_to_its_next_form() {
        let catalog = Catalog::open().expect("catalog");
        let links = catalog.hero_data_links("H30", "en");
        assert!(links.iter().any(|hit| hit.key == "hero:F23" && hit.aside == "Required spirit"));
        assert!(links.iter().any(|hit| hit.title == "Required intimacy" && hit.aside == "30,000"));
        let materials: Vec<_> = links.iter()
            .filter(|hit| hit.key.starts_with("item:"))
            .map(|hit| (hit.key.as_str(), hit.aside.as_str()))
            .collect();
        assert_eq!(materials, vec![
            ("item:6403", "× 30"),
            ("item:3506", "× 1"),
            ("item:5250", "× 1"),
        ]);
        assert!(links.iter().any(|hit| hit.key == "hero:V02" && hit.aside == "Next form"));
        assert!(!links.iter().any(|hit| hit.aside == "50,000" || hit.key == "item:8628" || hit.key == "item:9744"));
        assert!(catalog.hero_data_facts("H30", "en", None).iter().all(|(label, _)| label != "Evolution intimacy"));
        assert!(catalog.hero_data_facts("H30", "zh", None).iter().all(|(label, _)| label != "進化所需親密度"));
    }

    #[test]
    fn first_spirit_has_no_incoming_recipe_and_mount_acquisition_is_unchanged() {
        let catalog = Catalog::open().expect("catalog");
        let first = catalog.hero_data_links("F16", "en");
        assert!(!first.iter().any(|hit| hit.key.starts_with("companion:incoming-evolution:") || hit.key.starts_with("item:")));
        assert!(first.iter().any(|hit| hit.key == "hero:F19" && hit.aside == "Next form"));
        assert!(catalog.hero_data_facts("F16", "en", None).iter().all(|(label, _)| label != "Evolution intimacy"));
        let mount = catalog.hero_data_links("mount:3", "en");
        assert!(!mount.iter().any(|hit| hit.key.starts_with("companion:incoming-evolution:")));
        assert!(mount.iter().any(|hit| hit.key == "companion:acquisition"));
        assert!(mount.iter().any(|hit| hit.key == "hero:mount:1" && hit.aside == "Required mount"));
    }

    #[test]
    fn imported_companions_have_separate_bonus_stats_and_valid_links() {
        let catalog = Catalog::open().expect("catalog");
        let mounts: Vec<_> = catalog
            .index
            .heroes
            .iter()
            .filter(|hero| hero.rank == 13)
            .collect();
        let spirits: Vec<_> = catalog
            .index
            .heroes
            .iter()
            .filter(|hero| hero.rank == 14)
            .collect();
        assert_eq!(mounts.len(), 15);
        assert_eq!(spirits.len(), 33);
        for mount in &mounts {
            let stats = catalog.hero_stat_lines(&mount.code, "en");
            assert!(!stats.is_empty(), "mount {} bonuses", mount.code);
            assert!(stats.iter().all(|(label, _)| !matches!(
                label.as_str(),
                "HP" | "MP" | "STR" | "DEX" | "VIT" | "INT"
            )));
            let facts = catalog.hero_sheet(&mount.code, "en").unwrap().3;
            assert!(!facts.is_empty(), "mount {} requirements", mount.code);
            assert!(facts.iter().all(|fact| !stats.contains(fact)));
        }
        for spirit in &spirits {
            let stats = catalog.hero_stat_lines(&spirit.code, "en");
            assert!(
                stats.iter().any(|(label, _)| label == "HP"),
                "spirit {} HP",
                spirit.code
            );
        }
        let mut item_links = 0;
        let mut form_links = 0;
        for hero in mounts.into_iter().chain(spirits) {
            let mut section_keys = std::collections::HashSet::new();
            for link in catalog.hero_data_links(&hero.code, "en") {
                if link.chip == "section" {
                    assert!(!link.key.is_empty());
                    assert!(
                        section_keys.insert(link.key.clone()),
                        "duplicate section in {}",
                        hero.code
                    );
                }
                if let Some(code) = link.key.strip_prefix("item:") {
                    assert!(catalog.index.find_item(code.parse().unwrap()).is_some());
                    item_links += 1;
                }
                if let Some(code) = link.key.strip_prefix("hero:") {
                    assert!(catalog.index.heroes.iter().any(|hero| hero.code == code));
                    form_links += 1;
                }
            }
        }
        assert!(item_links > 0);
        assert!(form_links > 0);
    }

    #[test]
    fn directory_icons_are_available_and_stable_across_languages() {
        let catalog = Catalog::open().expect("catalog");
        let zh = catalog.index.hero_rank_hits("zh");
        let en = catalog.index.hero_rank_hits("en");
        for hit in zh {
            // The optional horse portrait atlas is not part of the bundled client.
            if hit.key != "rank:13" {
                assert!(
                    hit.icon.as_ref().is_some_and(|(path, _)| path.is_file()),
                    "{} portrait",
                    hit.key
                );
            }
            assert_eq!(
                hit.icon,
                en.iter().find(|other| other.key == hit.key).unwrap().icon
            );
        }
        for land in catalog.index.hero_land_hits("zh") {
            assert!(land.icon.is_some(), "{} portrait", land.key);
            for tier in catalog
                .index
                .hero_tier_hits(land.key.trim_start_matches("land:"), "zh")
            {
                if tier.key != "band:13" {
                    assert!(tier.icon.is_some(), "{} portrait", tier.key);
                }
            }
        }
    }
}
