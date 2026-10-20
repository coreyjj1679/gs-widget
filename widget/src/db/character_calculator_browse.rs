//! Complete, rank-based browsing for characters with calculable synthesis routes.
use std::collections::BTreeMap;

use super::character_calculator::CharacterCalcCandidate;
use super::hero::{hero_names, hero_rank_order};
use super::{hero_rank_label, ui_text, Catalog};

pub struct CharacterCalcGroup {
    /// Empty means all ranks; other IDs are the source rank number.
    pub id: String,
    pub title: String,
    pub count: usize,
}

impl Catalog {
    pub fn character_calculator_groups(&self, lang: &str) -> Vec<CharacterCalcGroup> {
        let mut counts = BTreeMap::<i64, usize>::new();
        for hero in &self.index.heroes {
            if self
                .character_calculator_entry_route(&hero.code, lang)
                .is_some()
            {
                *counts.entry(hero.rank).or_default() += 1;
            }
        }
        let mut ranks: Vec<_> = counts.keys().copied().collect();
        ranks.sort_by_key(|rank| (hero_rank_order(*rank), *rank));
        let mut groups = vec![CharacterCalcGroup {
            id: String::new(),
            title: ui_text(lang, "全部", "All"),
            count: counts.values().sum(),
        }];
        groups.extend(ranks.into_iter().map(|rank| {
            let id = rank.to_string();
            CharacterCalcGroup {
                title: hero_rank_label(&id, lang),
                id,
                count: counts[&rank],
            }
        }));
        groups
    }

    pub fn character_calculator_browse(
        &self,
        query: &str,
        group: &str,
        lang: &str,
    ) -> Vec<CharacterCalcCandidate> {
        let rank = if group.is_empty() {
            None
        } else {
            let Ok(rank) = group.parse::<i64>() else {
                return Vec::new();
            };
            Some(rank)
        };
        let query = query.trim();
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        let mut heroes: Vec<_> = self
            .index
            .heroes
            .iter()
            .filter(|hero| {
                rank.is_none_or(|rank| rank == hero.rank)
                    && (query.is_empty()
                        || self.index.name_hit(query, &folded, &hero.code, &hero.names))
            })
            .collect();
        heroes.sort_by_key(|hero| (hero_rank_order(hero.rank), hero.rank, hero.code.as_str()));
        heroes
            .into_iter()
            .filter_map(|hero| {
                self.character_calculator_entry_route(&hero.code, lang)?;
                Some(CharacterCalcCandidate {
                    code: hero.code.clone(),
                    title: hero_names(lang, hero).0,
                    subtitle: format!(
                        "{} · {}",
                        hero_rank_label(&hero.rank.to_string(), lang),
                        hero.code
                    ),
                    route_count: self.character_calculator_routes(&hero.code, lang).len(),
                    icon: hero.icon.clone(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn groups_partition_all_craftable_characters_without_search_limits() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let all = catalog.character_calculator_browse("", "", "zh");
        let codes: BTreeSet<_> = all.iter().map(|row| row.code.as_str()).collect();
        let expected: BTreeSet<_> = catalog
            .index
            .heroes
            .iter()
            .filter(|hero| {
                catalog
                    .character_calculator_entry_route(&hero.code, "zh")
                    .is_some()
            })
            .map(|hero| hero.code.as_str())
            .collect();
        assert!(all.len() > 80, "Browsing must not inherit the search limit");
        assert_eq!(codes.len(), all.len());
        assert_eq!(codes, expected);
        assert!(
            !codes.contains("U00"),
            "Recruitment estimates are not synthesis"
        );

        let groups = catalog.character_calculator_groups("zh");
        assert_eq!(groups[0].id, "");
        assert_eq!(groups[0].count, all.len());
        assert_eq!(
            groups
                .iter()
                .skip(1)
                .map(|group| group.count)
                .sum::<usize>(),
            all.len()
        );
        let mut grouped = BTreeSet::new();
        let mut previous_order = None;
        for group in groups.iter().skip(1) {
            let rank = group.id.parse::<i64>().unwrap();
            let order = (hero_rank_order(rank), rank);
            assert!(previous_order.is_none_or(|previous| previous <= order));
            previous_order = Some(order);
            let rows = catalog.character_calculator_browse("", &group.id, "zh");
            assert_eq!(rows.len(), group.count);
            assert!(!rows.is_empty());
            for row in rows {
                assert!(grouped.insert(row.code.clone()), "Duplicated character");
                let hero = catalog
                    .index
                    .heroes
                    .iter()
                    .find(|hero| hero.code == row.code)
                    .unwrap();
                assert_eq!(hero.rank, rank);
            }
        }
        assert_eq!(
            grouped,
            codes
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn named_characters_keep_their_source_rank_in_every_language() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let ranks = [
            ("T51", "9"),
            ("V09", "9"),
            ("E74", "8"),
            ("mount:2", "13"),
            ("F19", "14"),
        ];
        let baseline = catalog.character_calculator_groups("zh");
        for lang in ["zh", "en", "ko"] {
            let groups = catalog.character_calculator_groups(lang);
            assert_eq!(groups[0].title, ui_text(lang, "全部", "All"));
            assert_eq!(
                groups
                    .iter()
                    .map(|group| (&group.id, group.count))
                    .collect::<Vec<_>>(),
                baseline
                    .iter()
                    .map(|group| (&group.id, group.count))
                    .collect::<Vec<_>>()
            );
            for (code, rank) in ranks {
                let group = groups.iter().find(|group| group.id == rank).unwrap();
                assert_eq!(group.title, hero_rank_label(rank, lang));
                let rows = catalog.character_calculator_browse("", rank, lang);
                let row = rows.iter().find(|row| row.code == code).unwrap();
                let hero = catalog
                    .index
                    .heroes
                    .iter()
                    .find(|hero| hero.code == code)
                    .unwrap();
                assert_eq!(row.title, hero_names(lang, hero).0);
                assert_eq!(row.icon, hero.icon);
                assert!(row.route_count > 0);
                assert!(row.subtitle.starts_with(&group.title));
            }
        }
        assert_eq!(hero_rank_label("9", "zh"), "四天王");
        assert_eq!(hero_rank_label("9", "en"), "Four Kings");
        assert_eq!(hero_rank_label("9", "ko"), "사천왕");
    }

    #[test]
    fn query_filters_inside_the_selected_group_and_invalid_groups_are_empty() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let kings = catalog.character_calculator_browse(" T51 ", "9", "zh");
        assert_eq!(kings.len(), 1);
        assert_eq!(kings[0].code, "T51");
        assert!(catalog
            .character_calculator_browse("T51", "8", "zh")
            .is_empty());
        assert!(catalog
            .character_calculator_browse("", "not-a-rank", "zh")
            .is_empty());
        assert!(catalog
            .character_calculator_browse("", "999", "zh")
            .is_empty());
        assert!(catalog
            .character_calculator_browse("U00", "", "zh")
            .is_empty());

        let name = catalog
            .character_calculator_browse("", "9", "zh")
            .into_iter()
            .find(|row| row.code == "T51")
            .unwrap()
            .title;
        assert!(catalog
            .character_calculator_browse(&name, "9", "zh")
            .iter()
            .any(|row| row.code == "T51"));
        assert!(catalog
            .character_calculator_browse(&name, "8", "zh")
            .is_empty());
    }
}
