//! Full-mode crafting cost calculator. Shared documents carry their own recipe snapshot.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::{calculator_export, db, icons, window, App, CalcChoice, CalcMaterial, CalcSaved};

mod library;

type IconCache = Rc<RefCell<HashMap<String, Image>>>;
struct Controller {
    catalog: Rc<db::Catalog>,
    book: Result<db::CalcCatalog, String>,
    document: RefCell<Option<db::CalcDocument>>,
    recipes: RefCell<Vec<i64>>,
    cache: IconCache,
    load_error: Option<String>,
    library: RefCell<Result<db::CalcLibrary, String>>,
    folder_id: Cell<u64>,
    active_entry: Cell<Option<u64>>,
}

fn tr<'a>(english: bool, zh: &'a str, en: &'a str) -> &'a str {
    if english {
        en
    } else {
        zh
    }
}
fn number(value: i64) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    for (i, c) in text.chars().enumerate() {
        if i > 0 && (text.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
fn per_unit(total: i64, units: i64) -> String {
    // Fixed-point averaging avoids float precision loss for large game prices.
    let hundredths = (i128::from(total) * 100 + i128::from(units) / 2) / i128::from(units);
    let whole = (hundredths / 100) as i64;
    let fraction = hundredths % 100;
    if fraction == 0 {
        number(whole)
    } else {
        format!("{}.{fraction:02}", number(whole))
    }
}
fn parse_amount(input: &str, english: bool) -> Result<Option<i64>, String> {
    let text = input.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let groups: Vec<_> = text.split(',').collect();
    let valid = groups
        .iter()
        .all(|g| !g.is_empty() && g.bytes().all(|c| c.is_ascii_digit()))
        && (groups.len() == 1
            || (groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3)));
    if !valid {
        return Err(tr(
            english,
            "請輸入非負整數，例如 1,000；留空代表尚未填價。",
            "Enter a non-negative whole number, such as 1,000; leave blank for an unknown price.",
        )
        .into());
    }
    groups.concat().parse().map(Some).map_err(|_| {
        tr(
            english,
            "數量或單價過大。",
            "Quantity or unit price is too large.",
        )
        .into()
    })
}
fn update_rows<T: Clone + PartialEq + 'static>(current: ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    if let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() {
        if model.row_count() == rows.len() {
            for (index, row) in rows.into_iter().enumerate() {
                if model.row_data(index).as_ref() != Some(&row) {
                    model.set_row_data(index, row);
                }
            }
            return current;
        }
    }
    ModelRc::new(VecModel::from(rows))
}
fn recipe_label(source: &str, id: i64, output: i64, fee: i64, english: bool) -> String {
    format!(
        "{source} · #{id} · {} {} · {} {}",
        tr(english, "每批", "batch"),
        number(output),
        tr(english, "費用", "fee"),
        number(fee)
    )
}
fn notices(doc: &db::CalcDocument, english: bool) -> String {
    let mut notes = vec![tr(
        english,
        "單價由用家輸入，不代表市場價格。",
        "Unit prices are user estimates, not market quotes.",
    )
    .to_string()];
    match doc.success {
        Some(rate) if rate < 100 => notes.push(format!("{} {rate}%；{}", tr(english, "配方成功率", "Recipe success"), tr(english, "成本不含失敗重試。", "cost excludes failed attempts."))),
        None => notes.push(tr(english, "DB 未記錄成功率，成本按製作批次計，不含失敗重試。", "No success rate is recorded; costs cover planned batches, excluding failed attempts.").into()),
        _ => (),
    }
    if doc.learn_fee.is_some_and(|n| n > 0) {
        notes.push(
            tr(
                english,
                "不含配方學習費。",
                "Recipe learning fees are excluded.",
            )
            .into(),
        );
    }
    notes.join(" ")
}
impl Controller {
    fn icon(&self, code: i64) -> (Image, bool) {
        let icon = self
            .catalog
            .calculator_icon(code)
            .and_then(|(path, frame)| icons::cached_icon(&self.cache, &path, frame));
        let exists = icon.is_some();
        (icon.unwrap_or_default(), exists)
    }
    fn search(&self, app: &App, query: &str) {
        let Ok(book) = &self.book else { return };
        let lang = app.get_lang();
        let english = lang == "en-zh";
        let choices = self
            .catalog
            .calculator_items(book, query, &lang)
            .into_iter()
            .map(|item| {
                let (icon, has_icon) = self.icon(item.code);
                CalcChoice {
                    id: item.code.to_string().into(),
                    title: item.title.into(),
                    subtitle: format!("{} {}", item.recipe_count, tr(english, "種配方", "recipes"))
                        .into(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_calculator_candidates(ModelRc::new(VecModel::from(choices)));
    }
    fn render(&self, app: &App) {
        self.render_library(app);
        let lang = app.get_lang();
        let english = lang == "en-zh";
        let current = self.document.borrow();
        app.set_calculator_error(self.load_error.clone().unwrap_or_default().into());
        let Some(doc) = current.as_ref() else {
            app.set_calculator_selected_name("".into());
            app.set_calculator_selected_meta("".into());
            app.set_calculator_selected_has_icon(false);
            app.set_calculator_materials(ModelRc::default());
            app.set_calculator_recipes(ModelRc::default());
            app.set_calculator_share_enabled(false);
            app.set_calculator_complete(false);
            app.set_calculator_status("".into());
            app.set_calculator_notice("".into());
            if let Err(error) = &self.book {
                app.set_calculator_error(error.clone().into());
            }
            return;
        };
        let breakdown = match doc.calculate() {
            Ok(value) => value,
            Err(error) => {
                app.set_calculator_error(error.into());
                return;
            }
        };
        let (icon, has_icon) = self.icon(doc.item_code);
        app.set_calculator_selected_name(doc.title.clone().into());
        app.set_calculator_selected_icon(icon);
        app.set_calculator_selected_has_icon(has_icon);
        app.set_calculator_quantity(doc.quantity.to_string().into());
        app.set_calculator_selected_meta(
            format!(
                "{} · #{} · {} {} · {} {} · DB {}",
                doc.source,
                doc.recipe_id,
                number(breakdown.batches),
                tr(english, "批", "batches"),
                tr(english, "實際產出", "actual output"),
                number(breakdown.output_units),
                doc.catalog_version
            )
            .into(),
        );
        let mut recipe_ids = vec![doc.recipe_id];
        let mut recipe_labels = vec![recipe_label(
            &doc.source,
            doc.recipe_id,
            doc.output,
            doc.fee_per_batch,
            english,
        )
        .into()];
        if let Ok(book) = &self.book {
            for recipe in self.catalog.calculator_recipes(book, doc.item_code, &lang) {
                if recipe.recipe_id != doc.recipe_id {
                    recipe_ids.push(recipe.recipe_id);
                    recipe_labels.push(
                        recipe_label(
                            &recipe.source,
                            recipe.recipe_id,
                            recipe.output,
                            recipe.fee,
                            english,
                        )
                        .into(),
                    );
                }
            }
        }
        *self.recipes.borrow_mut() = recipe_ids;
        app.set_calculator_recipes(ModelRc::new(VecModel::from(recipe_labels)));
        app.set_calculator_recipe_index(0);
        let rows = breakdown
            .materials
            .iter()
            .map(|line| {
                let (icon, has_icon) = self.icon(line.key.code);
                CalcMaterial {
                    key: format!("{}:{}", line.key.code, line.key.enhance).into(),
                    title: line.title.clone().into(),
                    subtitle: if line.key.enhance > 0 {
                        format!(
                            "{} +{}",
                            tr(english, "強化等級", "Enhancement"),
                            line.key.enhance
                        )
                        .into()
                    } else {
                        "".into()
                    },
                    quantity: number(line.quantity).into(),
                    price: line.unit_price.map(number).unwrap_or_default().into(),
                    subtotal: line
                        .line_total
                        .map(number)
                        .unwrap_or_else(|| "—".into())
                        .into(),
                    missing_price: line.unit_price.is_none(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_calculator_materials(update_rows(app.get_calculator_materials(), rows));
        let complete = breakdown.total.is_some();
        app.set_calculator_complete(complete);
        app.set_calculator_share_enabled(true);
        app.set_calculator_material_total(number(breakdown.material_total).into());
        app.set_calculator_fee(number(breakdown.fee).into());
        app.set_calculator_total(number(breakdown.known_total).into());
        app.set_calculator_compact_total(number(breakdown.known_total).len() > 17);
        app.set_calculator_unit_total(
            breakdown
                .total
                .map(|n| per_unit(n, breakdown.output_units))
                .unwrap_or_else(|| "—".into())
                .into(),
        );
        app.set_calculator_status(
            if complete {
                format!(
                    "{} {} · {}",
                    breakdown.priced,
                    tr(english, "種材料已填價", "materials priced"),
                    tr(
                        english,
                        "單位：兩；每件成本按實際產出計算",
                        "Currency: coins; per-item cost uses actual output"
                    )
                )
            } else {
                format!(
                    "{} {} · {}",
                    breakdown.materials.len() - breakdown.priced,
                    tr(english, "種材料尚未填價", "materials still unpriced"),
                    tr(
                        english,
                        "0 代表免費，留空代表未知",
                        "0 means free; blank means unknown"
                    )
                )
            }
            .into(),
        );
        app.set_calculator_notice(notices(doc, english).into());
    }
    fn commit(&self, app: &App, next: db::CalcDocument, info: &str) -> bool {
        if let Some(error) = &self.load_error {
            app.set_calculator_error(error.clone().into());
            return false;
        }
        if let Err(error) = next.calculate().and_then(|_| db::save_calculator(&next)) {
            app.set_calculator_error(error.into());
            return false;
        }
        *self.document.borrow_mut() = Some(next);
        app.set_calculator_input_valid(true);
        self.render(app);
        app.set_calculator_info(info.into());
        true
    }
    fn select(&self, app: &App, recipe_id: i64, preserve_quantity: bool) {
        let Ok(book) = &self.book else { return };
        let mut next = match self
            .catalog
            .calculator_document(book, recipe_id, &app.get_lang())
        {
            Ok(doc) => doc,
            Err(error) => {
                app.set_calculator_error(error.into());
                return;
            }
        };
        if let Some(old) = self.document.borrow().as_ref() {
            if preserve_quantity {
                next.quantity = old.quantity;
            }
            for row in &mut next.materials {
                row.unit_price = old
                    .materials
                    .iter()
                    .find(|old| old.code == row.code && old.enhance == row.enhance)
                    .and_then(|old| old.unit_price);
            }
        }
        if self.commit(app, next, "") && !preserve_quantity {
            self.active_entry.set(None);
            self.render_library(app);
        }
    }
    fn card(&self, app: &App) -> Result<calculator_export::CardData, String> {
        let current = self.document.borrow();
        let doc = current
            .as_ref()
            .ok_or("請先選擇道具 / Select an item first")?;
        let costs = doc.calculate()?;
        let english = app.get_lang() == "en-zh";
        let rows = costs
            .materials
            .iter()
            .map(|line| calculator_export::CardRow {
                title: line.title.clone(),
                quantity: number(line.quantity),
                unit_price: line
                    .unit_price
                    .map(number)
                    .unwrap_or_else(|| tr(english, "待填", "Unpriced").into()),
                subtotal: line.line_total.map(number).unwrap_or_else(|| "—".into()),
                icon: self.icon(line.key.code).0,
                unpriced: line.unit_price.is_none(),
            })
            .collect();
        Ok(calculator_export::CardData {
            english,
            title: doc.title.clone(),
            subtitle: format!("{} · #{}", doc.source, doc.recipe_id),
            quantity: number(costs.output_units),
            fee: number(costs.fee),
            total_label: tr(
                english,
                if costs.total.is_some() {
                    "成本合計"
                } else {
                    "已知成本 · 尚有材料未填價"
                },
                if costs.total.is_some() {
                    "Total cost"
                } else {
                    "Known cost · unpriced materials remain"
                },
            )
            .into(),
            total: number(costs.known_total),
            per_unit: costs
                .total
                .map(|n| per_unit(n, costs.output_units))
                .unwrap_or_else(|| "—".into()),
            notice: if doc.success == Some(100) {
                tr(
                    english,
                    "用家自訂單價。成本不含配方學習費。",
                    "User-supplied prices. Recipe learning fees excluded.",
                )
                .into()
            } else {
                tr(
                    english,
                    "用家自訂單價。成本不含失敗重試或學習費。",
                    "User-supplied prices. Failed attempts and learning fees excluded.",
                )
                .into()
            },
            metadata: format!(
                "DB {} · {}",
                doc.catalog_version,
                tr(
                    english,
                    "離線配方快照 · 單位：兩",
                    "Offline recipe snapshot · Currency: coins"
                )
            ),
            icon: self.icon(doc.item_code).0,
            rows,
        })
    }
}

pub(crate) fn install(app: &App, catalog: Rc<db::Catalog>, cache: IconCache) {
    let (document, load_error) = match db::load_calculator() {
        Ok(doc) => (doc, None),
        Err(error) => (None, Some(error)),
    };
    let library = db::load_calculator_library();
    let active = library.as_ref().ok().and_then(|book| {
        book.folders.iter().find_map(|folder| {
            folder
                .entries
                .iter()
                .find(|entry| Some(&entry.document) == document.as_ref())
                .map(|entry| (folder.id, entry.id))
        })
    });
    let folder_id = active
        .map(|(folder, _)| folder)
        .or_else(|| {
            library
                .as_ref()
                .ok()
                .and_then(|book| book.folders.first().map(|folder| folder.id))
        })
        .unwrap_or_default();
    let controller = Rc::new(Controller {
        book: catalog.calculator_catalog(),
        catalog,
        document: RefCell::new(document),
        recipes: RefCell::new(Vec::new()),
        cache,
        load_error,
        library: RefCell::new(library),
        folder_id: Cell::new(folder_id),
        active_entry: Cell::new(active.map(|(_, entry)| entry)),
    });
    library::install_callbacks(app, &controller);
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_refresh(move || {
        if let Some(app) = weak.upgrade() {
            c.render(&app);
            c.search(&app, "");
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_search(move |query| {
        if let Some(app) = weak.upgrade() {
            c.search(&app, &query);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_select_item(move |code| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(code) = code.parse::<i64>() else {
            return;
        };
        let Ok(book) = &c.book else { return };
        if let Some(recipe) = c
            .catalog
            .calculator_recipes(book, code, &app.get_lang())
            .first()
        {
            c.select(&app, recipe.recipe_id, false);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_select_recipe(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let id = usize::try_from(index)
            .ok()
            .and_then(|i| c.recipes.borrow().get(i).copied());
        // Index 0 is the current snapshot, including imported recipes. Selecting
        // it again must not replace shared metadata with today's local DB.
        app.set_calculator_recipe_index(0);
        if let Some(id) = id {
            if c.document
                .borrow()
                .as_ref()
                .is_some_and(|doc| doc.recipe_id == id)
            {
                return;
            }
            c.select(&app, id, true);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_quantity_changed(move |text| {
        let Some(app) = weak.upgrade() else { return };
        let Some(mut doc) = c.document.borrow().clone() else {
            return;
        };
        match parse_amount(&text, app.get_lang() == "en-zh") {
            Ok(Some(value)) if value > 0 => {
                doc.quantity = value;
                let valid = c.commit(&app, doc, "");
                app.set_calculator_input_valid(valid);
            }
            _ => {
                app.set_calculator_input_valid(false);
                app.set_calculator_error(
                    tr(
                        app.get_lang() == "en-zh",
                        "製作數量必須是大於 0 的整數。",
                        "Crafting quantity must be a positive whole number.",
                    )
                    .into(),
                );
            }
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_price_changed(move |key, text| {
        let Some(app) = weak.upgrade() else { return };
        let Some(mut doc) = c.document.borrow().clone() else {
            return;
        };
        let Some((code, enhance)) = key.split_once(':') else {
            return;
        };
        let (Ok(code), Ok(enhance)) = (code.parse::<i64>(), enhance.parse::<i64>()) else {
            return;
        };
        let price = match parse_amount(&text, app.get_lang() == "en-zh") {
            Ok(price) => price,
            Err(error) => {
                app.set_calculator_input_valid(false);
                app.set_calculator_error(error.into());
                return;
            }
        };
        if let Some(row) = doc
            .materials
            .iter_mut()
            .find(|m| m.code == code && m.enhance == enhance)
        {
            row.unit_price = price;
        }
        let valid = c.commit(&app, doc, "");
        app.set_calculator_input_valid(valid);
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_copy_code(move || {
        let Some(app) = weak.upgrade() else { return };
        let current = c.document.borrow(); let Some(doc) = current.as_ref() else { return };
        match db::encode_calculator(doc) {
            Ok(code) => { window::copy_to_clipboard(&code); app.set_calculator_info(tr(app.get_lang() == "en-zh", "分享碼已複製。對方可在「匯入分享碼」貼上，離線還原這份成本清單。", "Code copied. Paste it into Import code to restore this cost sheet offline.").into()); }
            Err(error) => app.set_calculator_error(error.into()),
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_import_code(move |code| {
        let Some(app) = weak.upgrade() else { return };
        if code.trim().starts_with("GS-F") {
            c.import_folder(&app, &code);
            return;
        }
        match db::decode_calculator(&code) {
            Ok(doc) => {
                if c.commit(
                    &app,
                    doc,
                    tr(
                        app.get_lang() == "en-zh",
                        "已匯入離線成本清單，可繼續修改並儲存到收藏。",
                        "Imported the offline cost sheet. Edit it or save it to your collection.",
                    ),
                ) {
                    c.active_entry.set(None);
                    c.render_library(&app);
                    app.set_calculator_library_open(false);
                }
            }
            Err(error) => app.set_calculator_error(error.into()),
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_export_card(move || {
        let Some(app) = weak.upgrade() else { return };
        match c
            .card(&app)
            .and_then(|card| calculator_export::export_card(&card))
        {
            Ok(paths) => {
                if let Some(first) = paths.first() {
                    let open_result = calculator_export::open_export(first);
                    app.set_calculator_info(
                        format!(
                            "{} {} JPG · {}{}",
                            tr(app.get_lang() == "en-zh", "已匯出", "Exported"),
                            paths.len(),
                            first.parent().unwrap_or(first).display(),
                            open_result
                                .err()
                                .map(|e| format!("\n{e}"))
                                .unwrap_or_default()
                        )
                        .into(),
                    );
                }
            }
            Err(error) => app.set_calculator_error(error.into()),
        }
    });
    controller.render(app);
    controller.search(app, "");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prices_distinguish_blank_zero_and_valid_grouping() {
        assert_eq!(parse_amount("", false).unwrap(), None);
        assert_eq!(parse_amount("0", false).unwrap(), Some(0));
        assert_eq!(parse_amount(" 1,234,567 ", false).unwrap(), Some(1_234_567));
        for bad in [
            "-1",
            "1,23",
            "1,,000",
            "1.5",
            "1000,000",
            "9223372036854775808",
        ] {
            assert!(parse_amount(bad, false).is_err());
        }
    }
    #[test]
    fn unit_average_uses_fixed_point_rounding() {
        assert_eq!(per_unit(10, 3), "3.33");
        assert_eq!(per_unit(11, 3), "3.67");
        assert_eq!(per_unit(i64::MAX, 1), number(i64::MAX));
    }
}
