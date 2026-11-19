//! Match report identities to catalog records. Relational rows must use the owner
//! or ingredient code, never their import ID or a coincidentally matching number.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use slint::{ComponentHandle, Image, Timer, TimerMode};

use super::Entry;
use crate::{agf, db, icons, App};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) enum Subject {
    Item(i64),
    Set(i64),
    Monster(String),
    Npc(i64),
    Quest { source: String, code: String },
    Character(String),
}

impl Entry {
    pub(super) fn subject(&self) -> Option<Subject> {
        let mut key = self.key.split(" / ");
        let first = key.next()?;
        let ingredient = self
            .after
            .get("item_code")
            .or_else(|| self.before.get("item_code"))
            .and_then(serde_json::Value::as_i64)
            .filter(|code| *code > 0);
        match self.table.as_str() {
            "items"
            | "item_gear"
            | "item_affix"
            | "item_bases"
            | "enhance"
            | "item_formulas"
            | "item_formula_parts"
            | "item_set_members"
            | "enchant_equipment"
            | "enchant_recipes"
            | "enchant_recipe_targets" => first.parse().ok().map(Subject::Item),
            "item_sets" | "item_set_effects" => first.parse().ok().map(Subject::Set),
            "recipes"
            | "recipe_facility"
            | "recipe_facility_locations"
            | "recipe_material_choices"
            | "work_products" => key.next()?.parse().ok().map(Subject::Item),
            "recipe_materials" => ingredient
                .or_else(|| key.next()?.parse().ok())
                .map(Subject::Item),
            "enchant_materials" => ingredient.map(Subject::Item),
            "monsters" | "monster_drops" | "monster_locations" | "monster_categories"
            | "monster_sprites" | "monster_portraits" => Some(Subject::Monster(first.to_owned())),
            "npcs" => first.parse().ok().map(Subject::Npc),
            "quests" | "quest_steps" | "quest_lines" => Some(Subject::Quest {
                source: first.to_owned(),
                code: key.next()?.to_owned(),
            }),
            "characters"
            | "character_stats"
            | "character_skills"
            | "character_requirements"
            | "character_materials"
            | "character_companions"
            | "character_facts"
            | "character_upgrades" => Some(Subject::Character(first.to_owned())),
            _ => None,
        }
    }
}

type Location = (PathBuf, i32);
type Decoded = (String, Option<agf::Icon>);

pub(super) fn visible_frame(path: &Path, frame: i32) -> Option<agf::Icon> {
    let icon = agf::frame_rgba(path, frame).ok()?;
    icon.rgba
        .chunks_exact(4)
        .any(|pixel| pixel[3] != 0)
        .then_some(icon)
}

pub(super) struct Artwork {
    catalog: Rc<db::Catalog>,
    // Use the same uploaded image objects as other catalog pages.
    images: Rc<RefCell<HashMap<String, Image>>>,
    routes: RefCell<HashMap<Subject, Option<String>>>,
    locations: RefCell<HashMap<Subject, Option<(String, Location)>>>,
    ready: RefCell<HashMap<String, Option<Image>>>,
    waiting: RefCell<HashMap<String, HashSet<usize>>>,
    requests: Sender<(String, Location)>,
    results: Receiver<Decoded>,
    timer: Timer,
    window: slint::Weak<App>,
}

impl Artwork {
    pub(super) fn new(
        app: &App,
        catalog: Rc<db::Catalog>,
        images: Rc<RefCell<HashMap<String, Image>>>,
    ) -> Self {
        let (requests, jobs) = mpsc::channel::<(String, Location)>();
        let (finished, results) = mpsc::channel();
        std::thread::spawn(move || {
            for (key, (path, frame)) in jobs {
                let pixels = visible_frame(&path, frame);
                if finished.send((key, pixels)).is_err() {
                    break;
                }
            }
        });
        Self {
            catalog,
            images,
            routes: RefCell::default(),
            locations: RefCell::default(),
            ready: RefCell::default(),
            waiting: RefCell::default(),
            requests,
            results,
            timer: Timer::default(),
            window: app.as_weak(),
        }
    }

    pub(super) fn route(&self, subject: &Subject) -> Option<String> {
        self.routes
            .borrow_mut()
            .entry(subject.clone())
            .or_insert_with(|| {
                let key = match subject {
                    Subject::Item(code) => format!("item:{code}"),
                    Subject::Monster(code) => format!("monster:{code}"),
                    Subject::Set(code) => format!("set:{code}"),
                    Subject::Npc(code) => format!("npc:{code}"),
                    Subject::Character(code) => format!("hero:{code}"),
                    Subject::Quest { source, code } => {
                        return self.catalog.quest_record_key(source, code)
                    }
                };
                self.catalog.has_detail_key(&key).then_some(key)
            })
            .clone()
    }

    fn locate(&self, subject: &Subject) -> Option<Location> {
        match subject {
            Subject::Item(code) => self.catalog.calculator_icon(*code),
            Subject::Monster(code) => self.catalog.linked_icon(&format!("monster:{code}")),
            Subject::Character(code) => self.catalog.hero_portrait(code),
            Subject::Set(code) => self.catalog.set_icon(*code),
            Subject::Npc(code) => self.catalog.npc_icon(*code),
            Subject::Quest { source, code } => self.catalog.quest_giver_icon(source, code),
        }
    }

    pub(super) fn image(&self, subject: &Subject, entry: usize) -> Option<Image> {
        let (key, location) = self
            .locations
            .borrow_mut()
            .entry(subject.clone())
            .or_insert_with(|| {
                let location = self.locate(subject)?;
                Some((format!("{}#{}", location.0.display(), location.1), location))
            })
            .clone()?;
        if let Some(image) = self.ready.borrow().get(&key) {
            return image.clone();
        }
        let mut waiting = self.waiting.borrow_mut();
        if let Some(rows) = waiting.get_mut(&key) {
            rows.insert(entry);
            return None;
        }
        if self.requests.send((key.clone(), location)).is_err() {
            return None;
        }
        waiting.insert(key, HashSet::from([entry]));
        if !self.timer.running() {
            let handle = self.window.clone();
            self.timer
                .start(TimerMode::Repeated, Duration::from_millis(16), move || {
                    if let Some(app) = handle.upgrade() {
                        app.invoke_finish_game_update_icons();
                    }
                });
        }
        None
    }

    pub(super) fn finish(&self) -> HashSet<usize> {
        let mut painted = HashSet::new();
        let mut waiting = self.waiting.borrow_mut();
        for (key, decoded) in self.results.try_iter() {
            let image = decoded.map(|pixels| {
                self.images
                    .borrow_mut()
                    .entry(key.clone())
                    .or_insert_with(|| icons::image_from_icon(pixels))
                    .clone()
            });
            if let Some(rows) = waiting.remove(&key) {
                if image.is_some() {
                    painted.extend(rows);
                }
            }
            self.ready.borrow_mut().insert(key, image);
        }
        if waiting.is_empty() {
            self.timer.stop();
        }
        painted
    }

    pub(super) fn reset_targets(&self) {
        // Jobs remain reusable across filters and version pairs. Their previous model indices do not.
        for indices in self.waiting.borrow_mut().values_mut() {
            indices.clear();
        }
    }
}
