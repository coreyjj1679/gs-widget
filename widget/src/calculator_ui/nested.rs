//! Tree presentation and explicit recipe/saved-sheet reuse. Paths identify an
//! occurrence, so the same material in separate branches can have different prices.
use super::*;
use crate::CalcNestedOption;

pub(super) struct DisplayRow<'a> {
    pub key: String,
    pub depth: usize,
    pub context: String,
    pub line: &'a db::CalcLine,
}

pub(super) fn rows(costs: &db::CalcBreakdown) -> Vec<DisplayRow<'_>> {
    fn walk<'a>(
        costs: &'a db::CalcBreakdown,
        path: &mut Vec<usize>,
        ancestors: &mut Vec<String>,
        out: &mut Vec<DisplayRow<'a>>,
    ) {
        for (index, line) in costs.materials.iter().enumerate() {
            path.push(index);
            out.push(DisplayRow {
                key: path
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("/"),
                depth: ancestors.len(),
                context: ancestors.join(" › "),
                line,
            });
            if let Some(child) = &line.nested {
                ancestors.push(line.title.clone());
                walk(child, path, ancestors, out);
                ancestors.pop();
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(costs, &mut Vec::new(), &mut Vec::new(), &mut out);
    out
}

pub(super) fn parse_path(key: &str) -> Option<Vec<usize>> {
    if key.is_empty() || key.len() > 64 {
        return None;
    }
    let path: Vec<usize> = key
        .split('/')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        })
        .collect::<Option<_>>()?;
    (path.len() <= db::MAX_NESTED_DEPTH + 1).then_some(path)
}

pub(super) fn unit_price(line: &db::CalcLine, lang: &str) -> String {
    if let Some(child) = &line.nested {
        // Effective price includes the whole batches needed for this occurrence,
        // including surplus. Never feed this rounded display back into totals.
        child
            .total
            .map(|total| per_unit(total, line.quantity))
            .unwrap_or_else(|| tr(lang, "待填", "Unpriced").into())
    } else {
        line.unit_price.map(number).unwrap_or_default()
    }
}

pub(super) fn subtotal(line: &db::CalcLine) -> String {
    line.line_total.map(number).unwrap_or_else(|| {
        if line.nested.is_some() {
            format!("{} + ?", number(line.known_total))
        } else {
            "—".into()
        }
    })
}

pub(super) fn detail(line: &db::CalcLine, lang: &str) -> String {
    let Some(child) = &line.nested else {
        return String::new();
    };
    format!(
        "{} {} · {} {} · {} {} · {} {}",
        number(child.batches),
        tr(lang, "批", "batches"),
        tr(lang, "產出", "output"),
        number(child.output_units),
        tr(lang, "餘量", "surplus"),
        number(child.output_units - line.quantity),
        tr(lang, "含合成費", "includes crafting fee"),
        number(child.fee)
    )
}

/// Preserve entered prices for matching occurrences when changing recipe variant.
/// Nested snapshots are carried only for an explicit variant change of this item.
pub(super) fn preserve_sources(
    next: &mut db::CalcDocument,
    old: &db::CalcDocument,
    keep_nested: bool,
) -> Result<(), String> {
    let mut children = Vec::new();
    for (index, row) in next.materials.iter_mut().enumerate() {
        if let Some(previous) = old
            .materials
            .iter()
            .find(|previous| previous.code == row.code && previous.enhance == row.enhance)
        {
            row.unit_price = previous.unit_price;
            if keep_nested {
                if let Some(child) = &previous.nested {
                    children.push((index, child.as_ref().clone()));
                }
            }
        }
    }
    for (index, child) in children {
        next.set_nested(&[index], Some(child))?;
    }
    Ok(())
}

impl Controller {
    pub(super) fn can_reuse(&self, code: i64, enhance: i64) -> bool {
        enhance == 0
            && self.library.borrow().as_ref().is_ok_and(|book| {
                book.folders.iter().any(|folder| {
                    folder
                        .entries
                        .iter()
                        .any(|entry| entry.document.item_code == code)
                })
            })
    }

    fn nested_options(&self, app: &App, key: &str, reuse: bool) {
        app.set_calculator_nested_options(ModelRc::default());
        if !app.get_calculator_input_valid() {
            return;
        }
        let Some(path) = parse_path(key) else { return };
        let current = self.document.borrow();
        let Some(doc) = current.as_ref() else { return };
        let Some(material) = doc.ingredient_at(&path) else {
            return;
        };
        if material.enhance != 0 {
            return;
        }
        let lang_value = app.get_lang();
        let lang = lang_value.as_str();
        let choices = if reuse {
            match db::load_calculator_library() {
                Ok(latest) => {
                    *self.library.borrow_mut() = Ok(latest);
                }
                Err(error) => {
                    app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                    return;
                }
            }
            self.library
                .borrow()
                .as_ref()
                .ok()
                .into_iter()
                .flat_map(|book| book.folders.iter())
                .flat_map(|folder| folder.entries.iter().map(move |entry| (folder, entry)))
                .filter(|(_, entry)| entry.document.item_code == material.code)
                .map(|(folder, entry)| CalcNestedOption {
                    id: format!("saved:{}", entry.id).into(),
                    title: entry.name.clone().into(),
                    subtitle: format!(
                        "{} · {} · {} {} · {}",
                        folder.name,
                        self.catalog.calculator_source_label(entry.document.recipe_id, &entry.document.source, lang),
                        tr(lang, "每批產出", "output per batch"),
                        number(entry.document.output),
                        tr(
                            lang,
                            "複製單價，按所需數量重算",
                            "Copy prices; recalculate for the required quantity"
                        )
                    )
                    .into(),
                })
                .collect::<Vec<_>>()
        } else {
            self.book
                .as_ref()
                .ok()
                .into_iter()
                .flat_map(|book| {
                    self.catalog
                        .calculator_recipes(book, material.code, &app.get_lang())
                })
                .map(|recipe| CalcNestedOption {
                    id: format!("recipe:{}", recipe.recipe_id).into(),
                    title: format!("{} · #{}", recipe.source, recipe.recipe_id).into(),
                    subtitle: format!(
                        "{} {} · {} {}",
                        tr(lang, "每批產出", "output per batch"),
                        number(recipe.output),
                        tr(lang, "每批合成費", "fee per batch"),
                        number(recipe.fee)
                    )
                    .into(),
                })
                .collect::<Vec<_>>()
        };
        app.set_calculator_nested_options(ModelRc::new(VecModel::from(choices)));
    }

    fn choose_nested(&self, app: &App, key: &str, option: &str) {
        if !app.get_calculator_input_valid() {
            return;
        }
        let Some(path) = parse_path(key) else { return };
        let Some(mut next) = self.document.borrow().clone() else {
            return;
        };
        let Some((kind, id)) = option.split_once(':') else {
            return;
        };
        let lang_value = app.get_lang();
        let lang = lang_value.as_str();
        let child = match kind {
            "recipe" => {
                let Ok(id) = id.parse::<i64>() else { return };
                let Ok(book) = &self.book else { return };
                let previous = next
                    .ingredient_at(&path)
                    .and_then(|row| row.nested.as_deref());
                // Choosing the current recipe must not discard imported metadata
                // or entered descendant prices.
                if previous.is_some_and(|doc| doc.recipe_id == id) {
                    return;
                }
                let mut child = match self.catalog.calculator_document(book, id, &app.get_lang()) {
                    Ok(doc) => doc,
                    Err(error) => {
                        app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                        return;
                    }
                };
                if let Some(previous) = previous {
                    if let Err(error) = preserve_sources(&mut child, previous, true) {
                        app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                        return;
                    }
                }
                child
            }
            "saved" => {
                let Ok(id) = id.parse::<u64>() else { return };
                let child = self.library.borrow().as_ref().ok().and_then(|book| {
                    book.folders
                        .iter()
                        .flat_map(|folder| folder.entries.iter())
                        .find(|entry| entry.id == id)
                        .map(|entry| entry.document.clone())
                });
                let Some(child) = child else {
                    app.set_calculator_error(
                        tr(
                            lang,
                            "找不到這份收藏，請重新選擇。",
                            "This saved sheet is no longer available. Choose again.",
                        )
                        .into(),
                    );
                    return;
                };
                child
            }
            _ => return,
        };
        match next.set_nested(&path, Some(child)) {
            Ok(()) => {
                self.commit(app, next, tr(lang,
                "已展開子配方；成本按所需數量重算，可獨立修改單價。",
                "Sub-recipe expanded and recalculated for the required quantity. Its prices can be edited independently."));
            }
            Err(error) => app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into()),
        }
    }
}

pub(super) fn install_callbacks(app: &App, controller: &Rc<Controller>) {
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_request_nested_options(move |key, reuse| {
        if let Some(app) = weak.upgrade() {
            c.nested_options(&app, &key, reuse);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_select_nested(move |key, option| {
        if let Some(app) = weak.upgrade() {
            c.choose_nested(&app, &key, &option);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_clear_nested(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if !app.get_calculator_input_valid() {
            return;
        }
        let Some(path) = parse_path(&key) else { return };
        let Some(mut next) = c.document.borrow().clone() else {
            return;
        };
        match next.set_nested(&path, None) {
            Ok(()) => {
                c.commit(&app, next, "");
            }
            Err(error) => app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurrence_paths_are_bounded_and_unambiguous() {
        assert_eq!(parse_path("0/12/3"), Some(vec![0, 12, 3]));
        for invalid in [
            "",
            "/1",
            "1/",
            "1//2",
            "-1",
            "1:2",
            "<script>",
            "0/0/0/0/0/0/0/0/0/0",
        ] {
            assert!(parse_path(invalid).is_none(), "{invalid}");
        }
    }
    #[test]
    fn doll_projection_and_jpg_use_the_same_checked_tree() {
        let catalog = db::Catalog::open().unwrap();
        let book = catalog.calculator_catalog().unwrap();
        let mut doc = catalog.calculator_document(&book, 1020, "en-zh").unwrap();
        for row in &mut doc.materials {
            row.unit_price = Some(1);
        }
        for (code, recipe) in [(6944, 696), (6946, 653)] {
            let index = doc
                .materials
                .iter()
                .position(|row| row.code == code)
                .unwrap();
            let mut child = catalog.calculator_document(&book, recipe, "en-zh").unwrap();
            for row in &mut child.materials {
                row.unit_price = Some(1);
            }
            if code == 6946 {
                let index = child
                    .materials
                    .iter()
                    .position(|row| row.code == 5241)
                    .unwrap();
                let mut powder = catalog.calculator_document(&book, 2613, "en-zh").unwrap();
                for row in &mut powder.materials {
                    row.unit_price = Some(1);
                }
                child.set_nested(&[index], Some(powder)).unwrap();
            }
            doc.set_nested(&[index], Some(child)).unwrap();
        }
        let cost = doc.calculate().unwrap();
        let display = rows(&cost);
        assert_eq!(display.len(), 15);
        assert_eq!(
            display
                .iter()
                .map(|row| &row.key)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            15
        );
        for row in &display {
            let path = parse_path(&row.key).unwrap();
            assert_eq!(doc.ingredient_at(&path).unwrap().code, row.line.key.code);
            assert_eq!(row.depth, path.len() - 1);
        }
        let card = card_document(&doc, "en", |_| Image::default(), |_| String::new()).unwrap();
        assert_eq!(card.total, "141,000,402");
        assert_eq!(card.fee, "100,000,000");
        assert_eq!(card.rows.len(), display.len());
        for (card_row, row) in card.rows.iter().zip(&display) {
            assert_eq!(card_row.depth, row.depth);
            assert_eq!(card_row.subtotal, subtotal(row.line));
            assert_eq!(card_row.nested, row.line.nested.is_some());
        }
        let paths = calculator_export::export_card(&card).unwrap();
        assert!(paths.len() > 1);
        for path in &paths {
            let image = image::open(path).unwrap();
            assert_eq!((image.width(), image.height()), (1200, 630));
        }
        // Optional review fixtures are isolated from the user's real preferences.
        if let Some(directory) = std::env::var_os("GERSANG_NESTED_QA_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join("calculator.json"),
                serde_json::to_vec_pretty(&doc).unwrap(),
            )
            .unwrap();
            let mut library = db::CalcLibrary::default();
            library.rename_folder(1, "Nested examples").unwrap();
            let beads = doc
                .materials
                .iter()
                .find(|row| row.code == 6946)
                .unwrap()
                .nested
                .as_deref()
                .unwrap()
                .clone();
            library
                .save_entry(1, None, "Earth beads · saved prices", beads)
                .unwrap();
            std::fs::write(
                directory.join("calculator-library.json"),
                serde_json::to_vec_pretty(&library).unwrap(),
            )
            .unwrap();
            for (index, path) in paths.iter().enumerate() {
                std::fs::copy(path, directory.join(format!("doll-page-{}.jpg", index + 1)))
                    .unwrap();
            }
        }
    }
    #[test]
    fn changing_recipe_variant_preserves_nested_prices_and_rebatches() {
        // Offline variants keep this behavior independent of duplicated catalogue rows.
        let document = |item, material, output, per_batch| {
            serde_json::from_value::<db::CalcDocument>(serde_json::json!({
                "version": 1,
                "catalog_version": "offline",
                "language": "en-zh",
                "recipe_id": item,
                "item_code": item,
                "title": format!("Item {item}"),
                "source": "Snapshot",
                "output": output,
                "fee_per_batch": 0,
                "learn_fee": null,
                "success": null,
                "quantity": 1,
                "materials": [{
                    "code": material,
                    "enhance": 0,
                    "title": format!("Material {material}"),
                    "per_batch": per_batch,
                    "unit_price": null
                }]
            }))
            .unwrap()
        };
        let mut old = document(100, 200, 1, 3);
        old.materials[0].unit_price = Some(11);
        let mut child = document(200, 300, 4, 7);
        child.materials[0].unit_price = Some(7);
        old.set_nested(&[0], Some(child)).unwrap();

        let mut next = document(100, 200, 5, 6);
        next.recipe_id = 101;
        next.quantity = 11;
        let mut independent = next.clone();
        preserve_sources(&mut next, &old, true).unwrap();
        assert_eq!(next.materials[0].unit_price, Some(11));
        assert_eq!(next.ingredient_at(&[0, 0]).unwrap().unit_price, Some(7));
        let cost = next.calculate().unwrap();
        assert_eq!(cost.batches, 3);
        assert_eq!(cost.materials[0].quantity, 18);
        let child_cost = cost.materials[0].nested.as_ref().unwrap();
        assert_eq!((child_cost.batches, child_cost.output_units), (5, 20));
        assert_eq!(child_cost.materials[0].quantity, 35);
        assert_eq!(cost.total, Some(245));

        preserve_sources(&mut independent, &old, false).unwrap();
        assert!(independent.materials[0].nested.is_none());
        assert_eq!(independent.materials[0].unit_price, Some(11));
    }
}
