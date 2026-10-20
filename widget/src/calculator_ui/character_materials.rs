//! Item recipe snapshots inside a character calculation. The leading typed key
//! identifies the character input; the remaining path identifies an occurrence.
use super::*;
use crate::CalcNestedOption;

fn path(key: &str) -> Option<(&str, Vec<usize>)> {
    let (parent, rest) = key
        .split_once('/')
        .map_or((key, None), |(a, b)| (a, Some(b)));
    parent
        .strip_prefix("item:")?
        .parse::<i64>()
        .ok()
        .filter(|code| *code > 0)?;
    Some((
        parent,
        match rest {
            Some(rest) => nested::parse_path(rest)?,
            None => Vec::new(),
        },
    ))
}

fn target(doc: &db::CharacterCalcDocument, key: &str) -> Option<(i64, i64)> {
    let (parent, path) = path(key)?;
    if path.is_empty() {
        doc.inputs
            .iter()
            .find(|input| input.key == parent && input.kind == "item")?;
        Some((parent.strip_prefix("item:")?.parse().ok()?, 0))
    } else {
        let row = doc.nested.get(parent)?.ingredient_at(&path)?;
        Some((row.code, row.enhance))
    }
}

fn child_at<'a>(doc: &'a db::CharacterCalcDocument, key: &str) -> Option<&'a db::CalcDocument> {
    let (parent, path) = path(key)?;
    let child = doc.nested.get(parent)?;
    if path.is_empty() {
        Some(child)
    } else {
        child.ingredient_at(&path)?.nested.as_deref()
    }
}

fn replace(
    doc: &mut db::CharacterCalcDocument,
    key: &str,
    child: Option<db::CalcDocument>,
) -> Result<(), String> {
    let (parent, path) = path(key).ok_or("找不到材料 / Material path not found")?;
    if path.is_empty() {
        return doc.set_item_nested(parent, child);
    }
    let mut top = doc
        .nested
        .get(parent)
        .cloned()
        .ok_or("找不到材料 / Material path not found")?;
    let input = doc
        .inputs
        .iter()
        .find(|input| input.key == parent)
        .ok_or("找不到材料 / Material path not found")?;
    top.quantity = input
        .per_unit
        .checked_mul(doc.quantity)
        .ok_or("數量或成本過大 / Quantity or cost exceeds the supported range")?;
    top.set_nested(&path, child)?;
    doc.set_item_nested(parent, Some(top))
}

pub(super) fn set_price(
    doc: &mut db::CharacterCalcDocument,
    key: &str,
    value: Option<i64>,
) -> Result<(), String> {
    if key.contains('/') {
        let (parent, path) = path(key).ok_or("找不到材料 / Material path not found")?;
        let row = doc
            .nested
            .get_mut(parent)
            .and_then(|child| child.ingredient_at_mut(&path))
            .ok_or("找不到材料 / Material path not found")?;
        if row.nested.is_some() {
            return Err("請修改子配方的材料單價 / Edit the sub-recipe's material prices".into());
        }
        row.unit_price = value;
    } else {
        if doc.nested.contains_key(key) {
            return Err("請修改子配方的材料單價 / Edit the sub-recipe's material prices".into());
        }
        let row = doc
            .inputs
            .iter_mut()
            .find(|input| input.key == key)
            .ok_or("找不到成本項目 / Cost entry not found")?;
        row.unit_price = value;
    }
    Ok(())
}

pub(super) fn detail(child: &db::CalcBreakdown, required: i64, lang: &str) -> String {
    format!(
        "{} {} · {} {} · {} {} · {} {}",
        number(child.batches),
        tr(lang, "批", "batches"),
        tr(lang, "產出", "output"),
        number(child.output_units),
        tr(lang, "餘量", "surplus"),
        number(child.output_units - required),
        tr(lang, "含合成費", "includes crafting fee"),
        number(child.fee)
    )
}

impl CharacterController {
    pub(super) fn can_craft(&self, code: i64, enhance: i64) -> bool {
        enhance == 0 && self.catalog.calculator_entry_recipe(code, "zh").is_some()
    }

    pub(super) fn can_reuse(&self, code: i64, enhance: i64) -> bool {
        enhance == 0
            && self.library.borrow().as_ref().is_ok_and(|library| {
                library
                    .folders
                    .iter()
                    .flat_map(|folder| &folder.entries)
                    .any(|entry| entry.document.item_code == code)
            })
    }

    pub(super) fn nested_rows(
        &self,
        parent: &str,
        title: &str,
        costs: &db::CalcBreakdown,
        lang: &str,
    ) -> Vec<CharacterCalcMaterial> {
        nested::rows(costs)
            .into_iter()
            .map(|row| {
                let line = row.line;
                let key = format!("{parent}/{}", row.key);
                let context = if row.context.is_empty() {
                    title.to_string()
                } else {
                    format!("{title} › {}", row.context)
                };
                let (icon, has_icon) = self.image(
                    self.catalog
                        .character_calculator_icon(&format!("item:{}", line.key.code)),
                );
                let errors = self.errors.borrow();
                let draft = errors.draft(&InputField::Price(key.clone()));
                CharacterCalcMaterial {
                    key: key.into(),
                    title: line.title.clone().into(),
                    subtitle: if line.key.enhance > 0 {
                        format!(
                            "{} +{}",
                            tr(lang, "強化等級", "Enhancement"),
                            line.key.enhance
                        )
                    } else {
                        String::new()
                    }
                    .into(),
                    context: context.into(),
                    depth: row.depth as i32 + 1,
                    quantity: number(line.quantity).into(),
                    price: nested::unit_price(line, lang).into(),
                    draft: draft.unwrap_or_default().into(),
                    has_draft: draft.is_some(),
                    subtotal: nested::subtotal(line).into(),
                    missing_price: line.line_total.is_none(),
                    nested: line.nested.is_some(),
                    can_craft: row.depth < db::MAX_NESTED_DEPTH
                        && self.can_craft(line.key.code, line.key.enhance),
                    can_reuse: row.depth < db::MAX_NESTED_DEPTH
                        && self.can_reuse(line.key.code, line.key.enhance),
                    detail: nested::detail(line, lang).into(),
                    icon,
                    has_icon,
                }
            })
            .collect()
    }

    fn nested_options(&self, app: &App, key: &str, reuse: bool) {
        app.set_character_calculator_nested_options(ModelRc::default());
        if !app.get_character_calculator_input_valid() {
            return;
        }
        let current = self.document.borrow();
        let Some(doc) = current.as_ref() else { return };
        let Some((code, enhance)) = target(doc, key) else {
            return;
        };
        if enhance != 0 {
            return;
        }
        let lang = app.get_lang();
        let choices = if reuse {
            match db::load_calculator_library() {
                Ok(library) => *self.library.borrow_mut() = Ok(library),
                Err(error) => {
                    app.set_character_calculator_error(db::localized_error(&lang, &error).into());
                    return;
                }
            }
            self.library
                .borrow()
                .as_ref()
                .ok()
                .into_iter()
                .flat_map(|library| &library.folders)
                .flat_map(|folder| folder.entries.iter().map(move |entry| (folder, entry)))
                .filter(|(_, entry)| entry.document.item_code == code)
                .map(|(folder, entry)| CalcNestedOption {
                    id: format!("saved:{}", entry.id).into(),
                    title: entry.name.clone().into(),
                    subtitle: format!(
                        "{} · {} · {} {}",
                        folder.name,
                        self.catalog.calculator_source_label(
                            entry.document.recipe_id,
                            &entry.document.source,
                            &lang
                        ),
                        tr(&lang, "每批產出", "output per batch"),
                        number(entry.document.output)
                    )
                    .into(),
                })
                .collect::<Vec<_>>()
        } else {
            self.book
                .as_ref()
                .ok()
                .into_iter()
                .flat_map(|book| self.catalog.calculator_recipes(book, code, &lang))
                .map(|recipe| CalcNestedOption {
                    id: format!("recipe:{}", recipe.recipe_id).into(),
                    title: format!("{} · #{}", recipe.source, recipe.recipe_id).into(),
                    subtitle: format!(
                        "{} {} · {} {}",
                        tr(&lang, "每批產出", "output per batch"),
                        number(recipe.output),
                        tr(&lang, "每批合成費", "fee per batch"),
                        number(recipe.fee)
                    )
                    .into(),
                })
                .collect::<Vec<_>>()
        };
        app.set_character_calculator_nested_options(ModelRc::new(VecModel::from(choices)));
    }

    fn set_source(&self, app: &App, key: &str, option: &str) {
        if !app.get_character_calculator_input_valid() {
            return;
        }
        let Some(mut next) = self.document.borrow().clone() else {
            return;
        };
        let Some((code, enhance)) = target(&next, key) else {
            return;
        };
        if enhance != 0 {
            return;
        }
        let result = (|| {
            let child = if option == "manual" {
                None
            } else if let Some(id) = option
                .strip_prefix("recipe:")
                .and_then(|id| id.parse::<i64>().ok())
            {
                let previous = child_at(&next, key);
                if previous.is_some_and(|child| child.recipe_id == id) {
                    return Ok(());
                }
                let book = self.book.as_ref().map_err(Clone::clone)?;
                let mut child = self
                    .catalog
                    .calculator_document(book, id, &app.get_lang())?;
                if let Some(previous) = previous {
                    nested::preserve_sources(&mut child, previous, true)?;
                }
                Some(child)
            } else if let Some(id) = option
                .strip_prefix("saved:")
                .and_then(|id| id.parse::<u64>().ok())
            {
                // Read again at selection so a removed saved sheet cannot be
                // silently resurrected from the popup's earlier snapshot.
                let library = db::load_calculator_library()?;
                let child = library.folders.iter().flat_map(|folder| &folder.entries)
                    .find(|entry| entry.id == id && entry.document.item_code == code)
                    .map(|entry| entry.document.clone())
                    .ok_or("找不到這份收藏，請重新選擇。 / This saved sheet is no longer available. Choose again.")?;
                *self.library.borrow_mut() = Ok(library);
                Some(child)
            } else {
                return Err("找不到材料 / Material path not found".into());
            };
            replace(&mut next, key, child)?;
            self.persist(app, next)?;
            self.errors.borrow_mut().clear();
            app.set_character_calculator_input_revision(
                app.get_character_calculator_input_revision()
                    .wrapping_add(1),
            );
            self.render(app);
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            app.set_character_calculator_error(db::localized_error(&app.get_lang(), &error).into());
        }
    }
}

pub(super) fn install_callbacks(app: &App, controller: &Rc<CharacterController>) {
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_character_calculator_request_nested_options(move |key, reuse| {
        if let Some(app) = weak.upgrade() {
            c.nested_options(&app, &key, reuse);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_character_calculator_set_source(move |key, option| {
        if let Some(app) = weak.upgrade() {
            c.set_source(&app, &key, &option);
        }
    });
}
