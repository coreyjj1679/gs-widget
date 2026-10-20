//! Full-mode crafting cost calculator. Shared documents carry their own recipe snapshot.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use slint::{ComponentHandle, Image, Model, ModelRc, Timer, TimerMode, VecModel};

use crate::{calculator_export, db, icons, window, App, CalcChoice, CalcMaterial, CalcSaved};

mod library;
mod nested;
mod characters;

type IconCache = Rc<RefCell<HashMap<String, Image>>>;
struct ExportedCard {
    paths: Vec<std::path::PathBuf>,
    open_error: Option<String>,
}
type ExportResult = Result<ExportedCard, String>;
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
    input_errors: RefCell<InputErrors>,
    export_result: RefCell<Option<Receiver<ExportResult>>>,
    export_timer: Timer,
}

// A valid edit in one field must not hide another field's invalid draft or
// re-enable saving a snapshot that differs from the visible input.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum InputField {
    Quantity,
    Price(String),
}
struct InvalidDraft {
    text: String,
    error: String,
}
#[derive(Default)]
struct InputErrors(BTreeMap<InputField, InvalidDraft>);
impl InputErrors {
    fn record(&mut self, field: InputField, text: &str, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.0.remove(&field);
                true
            }
            Err(error) => {
                self.0.insert(
                    field,
                    InvalidDraft {
                        text: text.into(),
                        error,
                    },
                );
                false
            }
        }
    }
    fn first(&self) -> Option<&str> {
        self.0.values().next().map(|draft| draft.error.as_str())
    }
    fn draft(&self, field: &InputField) -> Option<&str> {
        self.0.get(field).map(|draft| draft.text.as_str())
    }
    fn clear(&mut self) {
        self.0.clear();
    }
}

fn tr<'a>(lang: &str, zh: &'a str, en: &'a str) -> &'a str {
    db::ui_text_ref(lang, zh, en)
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
fn parse_amount(input: &str, lang: &str) -> Result<Option<i64>, String> {
    let text = input.trim();
    if text.is_empty() {
        return Ok(None);
    }
    // Commas in this UI field are display separators. Inserting or deleting a
    // digit in a formatted price temporarily changes the group lengths (for
    // example, 1,000 -> 1,2000); normalize only after the edit is accepted.
    // Keep rejecting empty groups and anything except ASCII decimal digits.
    // Share/import validation remains in db::calculator and is unaffected.
    let groups: Vec<_> = text.split(',').collect();
    let valid = groups
        .iter()
        .all(|g| !g.is_empty() && g.bytes().all(|c| c.is_ascii_digit()));
    if !valid {
        return Err(tr(
            lang,
            "請輸入非負整數，例如 1,000；留空代表尚未填價。",
            "Enter a non-negative whole number, such as 1,000; leave blank for an unknown price.",
        )
        .into());
    }
    groups.concat().parse().map(Some).map_err(|_| {
        tr(
            lang,
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
fn recipe_label(source: &str, id: i64, output: i64, fee: i64, lang: &str) -> String {
    format!(
        "{source} · #{id} · {} {} · {} {}",
        tr(lang, "每批", "batch"),
        number(output),
        tr(lang, "費用", "fee"),
        number(fee)
    )
}
fn notices(doc: &db::CalcDocument, lang: &str) -> String {
    let mut notes = vec![tr(
        lang,
        "單價由用家輸入，不代表市場價格。",
        "Unit prices are user estimates, not market quotes.",
    )
    .to_string()];
    match doc.success {
        Some(rate) if rate < 100 => notes.push(format!("{} {rate}%；{}", tr(lang, "配方成功率", "Recipe success"), tr(lang, "成本不含失敗重試。", "cost excludes failed attempts."))),
        None => notes.push(tr(lang, "DB 未記錄成功率，成本按製作批次計，不含失敗重試。", "No success rate is recorded; costs cover planned batches, excluding failed attempts.").into()),
        _ => (),
    }
    if doc.learn_fee.is_some_and(|n| n > 0) {
        notes.push(
            tr(
                lang,
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
        let lang_value = app.get_lang();
        let lang = lang_value.as_str();
        let choices = self
            .catalog
            .calculator_items(book, query, &lang)
            .into_iter()
            .map(|item| {
                let (icon, has_icon) = self.icon(item.code);
                CalcChoice {
                    id: item.code.to_string().into(),
                    title: item.title.into(),
                    subtitle: format!("{} {}", item.recipe_count, tr(lang, "種配方", "recipes"))
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
        let lang_value = app.get_lang();
        let lang = lang_value.as_str();
        let current = self.document.borrow();
        self.publish_input_errors(app);
        let Some(doc) = current.as_ref() else {
            app.set_calculator_selected_name("".into());
            app.set_calculator_selected_meta("".into());
            app.set_calculator_selected_has_icon(false);
            app.set_calculator_materials(ModelRc::default());
            app.set_calculator_has_nested(false);
            app.set_calculator_recipes(ModelRc::default());
            app.set_calculator_share_enabled(false);
            app.set_calculator_complete(false);
            app.set_calculator_status("".into());
            app.set_calculator_notice("".into());
            if let Err(error) = &self.book {
                app.set_calculator_error(db::localized_error(&app.get_lang(), &error.clone()).into());
            }
            return;
        };
        let localized = self.catalog.localized_calculator_document(doc, lang);
        let doc = &localized;
        let breakdown = match doc.calculate() {
            Ok(value) => value,
            Err(error) => {
                app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
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
                tr(lang, "批", "batches"),
                tr(lang, "實際產出", "actual output"),
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
            lang,
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
                            lang,
                        )
                        .into(),
                    );
                }
            }
        }
        *self.recipes.borrow_mut() = recipe_ids;
        app.set_calculator_recipes(ModelRc::new(VecModel::from(recipe_labels)));
        app.set_calculator_recipe_index(0);
        let display_rows = nested::rows(&breakdown);
        app.set_calculator_has_nested(display_rows.iter().any(|row| row.line.nested.is_some()));
        let rows = display_rows
            .iter()
            .map(|row| {
                let line = row.line;
                let (icon, has_icon) = self.icon(line.key.code);
                let can_expand = line.key.enhance == 0
                    && row.depth < db::MAX_NESTED_DEPTH
                    && self.book.as_ref().is_ok_and(|book| {
                        let recipe_count = self
                            .catalog
                            .calculator_recipes(book, line.key.code, &lang)
                            .len();
                        recipe_count > usize::from(line.nested.is_some())
                    });
                let errors = self.input_errors.borrow();
                let draft = errors.draft(&InputField::Price(row.key.clone()));
                CalcMaterial {
                    key: row.key.clone().into(),
                    draft: draft.unwrap_or_default().into(),
                    has_draft: draft.is_some(),
                    title: line.title.clone().into(),
                    subtitle: if line.key.enhance > 0 {
                        format!(
                            "{} +{}",
                            tr(lang, "強化等級", "Enhancement"),
                            line.key.enhance
                        )
                        .into()
                    } else {
                        "".into()
                    },
                    quantity: number(line.quantity).into(),
                    price: nested::unit_price(line, lang).into(),
                    subtotal: nested::subtotal(line).into(),
                    missing_price: line.line_total.is_none(),
                    depth: row.depth as i32,
                    nested: line.nested.is_some(),
                    can_expand,
                    can_reuse: row.depth < db::MAX_NESTED_DEPTH
                        && self.can_reuse(line.key.code, line.key.enhance),
                    detail: nested::detail(line, lang).into(),
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
                    breakdown.priced_leaves,
                    tr(lang, "種基礎材料已填價", "leaf materials priced"),
                    tr(
                        lang,
                        "單位：兩；每件成本按實際產出計算",
                        "Currency: coins; per-item cost uses actual output"
                    )
                )
            } else {
                format!(
                    "{} {} · {}",
                    breakdown.leaf_count - breakdown.priced_leaves,
                    tr(
                        lang,
                        "種基礎材料尚未填價",
                        "leaf materials still unpriced"
                    ),
                    tr(
                        lang,
                        "0 代表免費，留空代表未知",
                        "0 means free; blank means unknown"
                    )
                )
            }
            .into(),
        );
        let mut notice = notices(doc, lang);
        if breakdown.materials.iter().any(|line| line.nested.is_some()) {
            notice.push(' ');
            notice.push_str(tr(lang,
                "子配方同樣不含失敗重試及學習費；子材料單價按目前所需數量分攤整批費用，包含餘量。",
                "Sub-recipes also exclude failed attempts and learning fees. Computed material prices allocate whole-batch costs, including surplus, to the required quantity."));
        }
        app.set_calculator_notice(notice.into());
    }
    fn publish_input_errors(&self, app: &App) {
        let errors = self.input_errors.borrow();
        let error = self.load_error.as_deref().or_else(|| errors.first());
        app.set_calculator_input_valid(error.is_none());
        app.set_calculator_error(db::localized_error(&app.get_lang(), &error.unwrap_or_default()).into());
        let quantity = errors.draft(&InputField::Quantity);
        app.set_calculator_quantity_draft(quantity.unwrap_or_default().into());
        app.set_calculator_quantity_has_draft(quantity.is_some());
        // Drafts live with the controller, not only the transient editor, so
        // visiting Saved, Settings, or another page cannot hide invalid input.
        // Publish changed metadata without recalculating costs or rebuilding rows.
        let rows = app.get_calculator_materials();
        for index in 0..rows.row_count() {
            let Some(mut row) = rows.row_data(index) else {
                continue;
            };
            let draft = errors.draft(&InputField::Price(row.key.to_string()));
            let text = draft.unwrap_or_default();
            if row.has_draft != draft.is_some() || row.draft.as_str() != text {
                row.has_draft = draft.is_some();
                row.draft = text.into();
                rows.set_row_data(index, row);
            }
        }
    }
    fn record_input(
        &self,
        app: &App,
        field: InputField,
        text: &str,
        result: Result<(), String>,
    ) -> bool {
        let valid = self.input_errors.borrow_mut().record(field, text, result);
        self.publish_input_errors(app);
        valid
    }
    fn commit_input(
        &self,
        app: &App,
        field: InputField,
        text: &str,
        next: db::CalcDocument,
    ) -> bool {
        // Remove this field's previous failure before render publishes the new
        // value, so an old invalid draft cannot overwrite a corrected edit.
        // Any calculation or persistence failure below records the new draft.
        self.input_errors
            .borrow_mut()
            .record(field.clone(), text, Ok(()));
        let result = if self.persist_document(app, next, "") {
            Ok(())
        } else {
            Err(app.get_calculator_error().to_string())
        };
        self.record_input(app, field, text, result)
    }
    fn commit(&self, app: &App, next: db::CalcDocument, info: &str) -> bool {
        if !self.persist_document(app, next, info) {
            return false;
        }
        // Selecting/importing a document replaces every raw input draft. Price
        // edits use commit_input instead so errors in other fields survive.
        self.input_errors.borrow_mut().clear();
        self.publish_input_errors(app);
        app.set_calculator_input_revision(app.get_calculator_input_revision().wrapping_add(1));
        true
    }
    fn persist_document(&self, app: &App, next: db::CalcDocument, info: &str) -> bool {
        if let Some(error) = &self.load_error {
            app.set_calculator_error(db::localized_error(&app.get_lang(), &error.clone()).into());
            return false;
        }
        // Valid edits are already committed on each keystroke. Enter or focus
        // loss should only format the field, not save and rebuild the same
        // document again while the user is clicking the next control.
        if self.document.borrow().as_ref() == Some(&next) {
            app.set_calculator_info(info.into());
            return true;
        }
        if let Err(error) = next.calculate().and_then(|_| db::save_calculator(&next)) {
            app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
            return false;
        }
        *self.document.borrow_mut() = Some(next);
        self.render(app);
        app.set_calculator_info(info.into());
        true
    }
    fn select(&self, app: &App, recipe_id: i64, preserve_quantity: bool) -> bool {
        let book = match &self.book {
            Ok(book) => book,
            Err(error) => {
                app.set_calculator_error(db::localized_error(&app.get_lang(), error).into());
                return false;
            }
        };
        let mut next = match self
            .catalog
            .calculator_document(book, recipe_id, &app.get_lang())
        {
            Ok(doc) => doc,
            Err(error) => {
                app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                return false;
            }
        };
        if let Some(old) = self.document.borrow().as_ref() {
            if preserve_quantity {
                next.quantity = old.quantity;
            }
            let keep_nested = preserve_quantity && old.item_code == next.item_code;
            if let Err(error) = nested::preserve_sources(&mut next, old, keep_nested) {
                app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                return false;
            }
        }
        if !self.commit(app, next, "") {
            return false;
        }
        if !preserve_quantity {
            self.active_entry.set(None);
            self.render_library(app);
        }
        true
    }
    fn card(&self, app: &App) -> Result<calculator_export::CardData, String> {
        let current = self.document.borrow();
        let doc = current
            .as_ref()
            .ok_or("請先選擇道具 / Select an item first")?;
        card_document(&self.catalog.localized_calculator_document(doc, &app.get_lang()), &app.get_lang(), |code| self.icon(code).0)
    }

    fn export_card(self: &Rc<Self>, app: &App) {
        if app.get_calculator_export_busy() || !app.get_calculator_input_valid() {
            return;
        }
        app.set_calculator_info("".into());
        app.set_calculator_error("".into());
        // Snapshot the current sheet and its small icons on the UI thread.
        // Font discovery, rasterization, JPEG encoding and file writes run below
        // with owned pixels, never with Slint images or a live document borrow.
        let card = match self.card(app) {
            Ok(card) => card,
            Err(error) => {
                app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        app.set_calculator_export_busy(true);
        if let Err(error) = std::thread::Builder::new()
            .name("calculator-jpg-export".into())
            .spawn(move || {
                let result = calculator_export::export_card(&card).map(|paths| {
                    let open_error = paths.first().and_then(|path| calculator_export::open_export(path).err());
                    ExportedCard { paths, open_error }
                });
                let _ = tx.send(result);
            })
        {
            app.set_calculator_export_busy(false);
            app.set_calculator_error(format!("{} {error}", tr(&app.get_lang(),
                "無法開始匯出 JPG，請重試。", "Could not start JPG export. Try again.")).into());
            return;
        }
        *self.export_result.borrow_mut() = Some(rx);
        let weak_app = app.as_weak();
        let weak_controller = Rc::downgrade(self);
        self.export_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let (Some(app), Some(controller)) = (weak_app.upgrade(), weak_controller.upgrade()) else {
                return;
            };
            let result = {
                let receiver = controller.export_result.borrow();
                let Some(receiver) = receiver.as_ref() else { return };
                match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err(tr(&app.get_lang(),
                        "JPG 匯出已中斷，請重新匯出。", "JPG export stopped unexpectedly. Try Export JPG again.").into()),
                }
            };
            controller.export_result.borrow_mut().take();
            controller.export_timer.stop();
            app.set_calculator_export_busy(false);
            // A retry in the calculator must also resolve an earlier export
            // failure that was reported globally while this pane was closed.
            let publish_global = !app.get_calculator_open()
                || app.get_feedback_source() == "calculator-export";
            match result {
                Ok(ExportedCard { paths, open_error }) => {
                    if let Some(first) = paths.first() {
                        app.set_calculator_info(format!("{} {} JPG · {}",
                            tr(&app.get_lang(), "已匯出", "Exported"),
                            paths.len(), first.parent().unwrap_or(first).display()).into());
                        // Do not hide invalid input entered while the snapshot was exporting.
                        controller.publish_input_errors(&app);
                        if let Some(error) = open_error {
                            app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into());
                            if publish_global {
                                crate::feedback::error_for(&app, "calculator-export",
                                    "JPG 已儲存，但無法開啟圖片。請返回合成成本查看儲存位置。",
                                    "JPG saved, but the image could not be opened. Open Crafting cost to see its location.",
                                    "JPG를 저장했지만 이미지를 열 수 없습니다. 제작 비용에서 저장 위치를 확인하세요.");
                            }
                        } else if publish_global {
                            let directory = first.parent().unwrap_or(first).display();
                            crate::feedback::success_for(&app, "calculator-export",
                                &format!("已匯出 {} JPG · {directory}", paths.len()),
                                &format!("Exported {} JPG · {directory}", paths.len()),
                                &format!("JPG {}개 내보내기 완료 · {directory}", paths.len()));
                        }
                    }
                }
                Err(error) => {
                    app.set_calculator_info("".into());
                    app.set_calculator_error(format!("{}\n{}",
                        db::localized_error(&app.get_lang(), &error),
                        tr(&app.get_lang(), "成本清單已保留，可再次按「匯出 JPG」重試。",
                            "Your cost sheet is unchanged. Try Export JPG again.")).into());
                    if publish_global {
                        crate::feedback::error_for(&app, "calculator-export",
                            "JPG 匯出失敗，成本清單已保留。請返回合成成本查看詳情並重試。",
                            "JPG export failed. Your cost sheet is unchanged. Open Crafting cost for details and retry.",
                            "JPG 내보내기에 실패했습니다. 비용표는 유지됩니다. 제작 비용에서 자세한 내용을 확인하고 다시 시도하세요.");
                    }
                }
            }
        });
    }
}

fn card_icon(icon: Image) -> Option<image::RgbaImage> {
    let pixels = icon.to_rgba8()?;
    image::RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.as_bytes().to_vec())
}

fn copy_share_code(app: &App, code: &str) -> bool {
    app.set_calculator_info("".into());
    if window::try_copy_to_clipboard(code) {
        true
    } else {
        app.set_calculator_error(tr(&app.get_lang(),
            "無法複製分享碼到剪貼簿。請重試；成本清單已保留。",
            "Could not copy the share code to the clipboard. Try again; your cost sheet is unchanged.").into());
        false
    }
}

fn card_document(
    doc: &db::CalcDocument,
    lang: &str,
    icon_for: impl Fn(i64) -> Image,
) -> Result<calculator_export::CardData, String> {
    let costs = doc.calculate()?;
    let rows = nested::rows(&costs)
        .into_iter()
        .map(|row| {
            let line = row.line;
            calculator_export::CardRow {
                title: line.title.clone(),
                quantity: number(line.quantity),
                unit_price: if line.nested.is_none() && line.unit_price.is_none() {
                    tr(lang, "待填", "Unpriced").into()
                } else {
                    nested::unit_price(line, lang)
                },
                subtotal: nested::subtotal(line),
                icon: card_icon(icon_for(line.key.code)),
                unpriced: line.line_total.is_none(),
                depth: row.depth,
                context: row.context,
                nested: line.nested.is_some(),
                detail: nested::detail(line, lang),
            }
        })
        .collect();
    Ok(calculator_export::CardData {
        language: lang.into(),
        title: doc.title.clone(),
        subtitle: format!("{} · #{}", doc.source, doc.recipe_id),
        quantity: number(costs.output_units),
        fee: number(costs.fee),
        total_label: tr(
            lang,
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
        notice: if doc.success == Some(100)
            && !costs.materials.iter().any(|line| line.nested.is_some())
        {
            tr(
                lang,
                "用家自訂單價。成本不含配方學習費。",
                "User-supplied prices. Recipe learning fees excluded.",
            )
            .into()
        } else {
            tr(
                lang,
                "用家自訂單價。成本不含失敗重試或學習費。",
                "User-supplied prices. Failed attempts and learning fees excluded.",
            )
            .into()
        },
        metadata: format!(
            "DB {} · {}",
            doc.catalog_version,
            tr(
                lang,
                "離線配方快照 · 單位：兩",
                "Offline recipe snapshot · Currency: coins"
            )
        ),
        icon: card_icon(icon_for(doc.item_code)),
        rows,
    })
}

pub(crate) fn install(app: &App, catalog: Rc<db::Catalog>, cache: IconCache) {
    let characters = characters::install(app, Rc::clone(&catalog), Rc::clone(&cache));
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
        input_errors: RefCell::default(),
        export_result: RefCell::default(),
        export_timer: Timer::default(),
    });
    library::install_callbacks(app, &controller);
    nested::install_callbacks(app, &controller);
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculate_record(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if !c.catalog.can_calculate_record(&key) {
            crate::feedback::error_for(&app, "calculator-navigation",
                "此項目沒有可計算的合成配方。",
                "No calculable crafting recipe is available for this record.",
                "이 항목에는 계산 가능한 제작법이 없습니다.");
            return;
        }
        let (opened, tab, error) = if let Some(code) = key.strip_prefix("item:").and_then(|code| code.parse().ok()) {
            let Some(recipe) = c.catalog.calculator_entry_recipe(code, &app.get_lang()) else { return };
            let current = c.document.borrow().as_ref().is_some_and(|document| document.item_code == code);
            let opened = if current && c.load_error.is_none() {
                c.render(&app);
                true
            } else {
                c.select(&app, recipe, false)
            };
            (opened, 0, app.get_calculator_error())
        } else if let Some(code) = key.strip_prefix("hero:") {
            (characters.open_record(&app, code), 1, app.get_character_calculator_error())
        } else {
            return;
        };
        if opened {
            if app.get_feedback_source() == "calculator-navigation" {
                app.set_feedback_message("".into());
                app.set_feedback_error(false);
            }
            app.invoke_show_record_calculator(tab);
        } else {
            // Keep the record visible when a draft cannot be saved. Navigating
            // anyway would show the previously selected, unrelated calculation.
            crate::feedback::error_for(&app, "calculator-navigation", &error, &error, &error);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_refresh(move || {
        if let Some(app) = weak.upgrade() {
            app.invoke_character_calculator_refresh();
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
        let Some(app) = weak.upgrade() else {
            return false;
        };
        let Some(mut doc) = c.document.borrow().clone() else {
            return false;
        };
        let lang_value = app.get_lang();
        let lang = lang_value.as_str();
        let value = match parse_amount(&text, lang) {
            Ok(Some(value)) if value > 0 => value,
            result => {
                let error = result.err().unwrap_or_else(|| {
                    tr(
                        lang,
                        "製作數量必須是大於 0 的整數。",
                        "Crafting quantity must be a positive whole number.",
                    )
                    .into()
                });
                return c.record_input(&app, InputField::Quantity, &text, Err(error));
            }
        };
        doc.quantity = value;
        c.commit_input(&app, InputField::Quantity, &text, doc)
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_price_changed(move |key, text| {
        let Some(app) = weak.upgrade() else {
            return false;
        };
        let Some(mut doc) = c.document.borrow().clone() else {
            return false;
        };
        let Some(path) = nested::parse_path(&key) else {
            return false;
        };
        let field = InputField::Price(key.to_string());
        let price = match parse_amount(&text, &app.get_lang()) {
            Ok(price) => price,
            Err(error) => return c.record_input(&app, field, &text, Err(error)),
        };
        let Some(row) = doc.ingredient_at_mut(&path) else {
            return false;
        };
        // An expanded subtotal is computed from its children, never overwritten.
        if row.nested.is_some() {
            return false;
        }
        row.unit_price = price;
        c.commit_input(&app, field, &text, doc)
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_copy_code(move || {
        let Some(app) = weak.upgrade() else { return };
        if !app.get_calculator_input_valid() { return; }
        app.set_calculator_info("".into());
        let current = c.document.borrow(); let Some(doc) = current.as_ref() else { return };
        match db::encode_calculator(doc) {
            Ok(code) => {
                if copy_share_code(&app, &code) {
                    c.publish_input_errors(&app);
                    app.set_calculator_info(tr(&app.get_lang(), "分享碼已複製。對方可在「匯入分享碼」貼上，離線還原這份成本清單。", "Code copied. Paste it into Import code to restore this cost sheet offline.").into());
                }
            }
            Err(error) => app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into()),
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
                        &app.get_lang(),
                        "已匯入離線成本清單，可繼續修改並儲存到收藏。",
                        "Imported the offline cost sheet. Edit it or save it to your collection.",
                    ),
                ) {
                    c.active_entry.set(None);
                    c.render_library(&app);
                    app.set_calculator_library_open(false);
                }
            }
            Err(error) => app.set_calculator_error(db::localized_error(&app.get_lang(), &error).into()),
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(&controller);
    app.on_calculator_export_card(move || {
        let Some(app) = weak.upgrade() else { return };
        c.export_card(&app);
    });
    controller.render(app);
    controller.search(app, "");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn price_edits_keep_blank_unknown_and_zero_free() {
        assert_eq!(parse_amount("", "zh").unwrap(), None);
        assert_eq!(parse_amount(" ", "zh").unwrap(), None);
        assert_eq!(parse_amount("0", "zh").unwrap(), Some(0));
        assert_eq!(parse_amount("0,000", "zh").unwrap(), Some(0));
        assert_eq!(parse_amount(" 1,234,567 ", "zh").unwrap(), Some(1_234_567));
    }
    #[test]
    fn formatted_price_accepts_insertion_and_deletion_between_commas() {
        // Editing any group must retain the new value even before commas have
        // been regrouped by the formatter on Enter or focus loss.
        for (edit, expected, formatted) in [
            ("1,2000", 12_000, "12,000"),
            ("1,00", 100, "100"),
            ("1000,000", 1_000_000, "1,000,000"),
            ("1,2345,678", 12_345_678, "12,345,678"),
            ("1,23,567", 123_567, "123,567"),
            (
                "9,223,372,036,854,775,807",
                i64::MAX,
                "9,223,372,036,854,775,807",
            ),
        ] {
            assert_eq!(parse_amount(edit, "zh").unwrap(), Some(expected));
            assert_eq!(number(expected), formatted);
            assert_eq!(parse_amount(formatted, "en").unwrap(), Some(expected));
        }
    }
    #[test]
    fn price_edits_reject_junk_ambiguous_separators_and_overflow() {
        for bad in [
            "-1",
            "+1",
            ",",
            ",123",
            "123,",
            "1,,000",
            "1.5",
            "1 000",
            "1, 000",
            "1e3",
            "12a,000",
            "１２３",
            "١٢٣",
            "1，000",
            "9223372036854775808",
            "9,223,372,036,854,775,808",
        ] {
            assert!(parse_amount(bad, "zh").is_err(), "accepted {bad:?}");
        }
    }
    #[test]
    fn correcting_one_draft_keeps_other_fields_invalid_until_corrected() {
        let mut errors = InputErrors::default();
        let first_price = InputField::Price("0".into());
        let nested_price = InputField::Price("1/0".into());
        assert!(!errors.record(first_price.clone(), "12a", Err("Invalid price".into())));
        assert!(!errors.record(InputField::Quantity, "", Err("Invalid quantity".into())));
        assert!(!errors.record(
            nested_price.clone(),
            "9,000,000",
            Err("Cost overflow".into())
        ));
        assert_eq!(errors.first(), Some("Invalid quantity"));

        assert!(errors.record(InputField::Quantity, "1", Ok(())));
        assert_eq!(errors.first(), Some("Invalid price"));
        assert_eq!(errors.draft(&first_price), Some("12a"));
        assert!(errors.record(first_price.clone(), "12", Ok(())));
        assert_eq!(errors.draft(&first_price), None);
        assert_eq!(errors.first(), Some("Cost overflow"));
        // Even a syntactically correct price must stay invalid if saving fails.
        assert!(!errors.record(nested_price.clone(), "100", Err("Cannot save draft".into())));
        assert_eq!(errors.first(), Some("Cannot save draft"));
        assert_eq!(errors.draft(&nested_price), Some("100"));
        assert!(errors.record(nested_price, "100", Ok(())));
        assert_eq!(errors.first(), None);
    }
    #[test]
    fn invalid_drafts_can_be_restored_until_document_is_replaced() {
        let mut errors = InputErrors::default();
        let price = InputField::Price("0/1".into());
        errors.record(InputField::Quantity, "", Err("Invalid quantity".into()));
        errors.record(price.clone(), "1,,000", Err("Invalid price".into()));
        // Recreating the UI must recover both the text and the fact that a
        // blank quantity is an invalid draft, not an absent draft.
        assert_eq!(errors.draft(&InputField::Quantity), Some(""));
        assert_eq!(errors.draft(&price), Some("1,,000"));
        errors.clear();
        assert_eq!(errors.first(), None);
        assert_eq!(errors.draft(&InputField::Quantity), None);
        assert_eq!(errors.draft(&price), None);
    }
    #[test]
    fn unit_average_uses_fixed_point_rounding() {
        assert_eq!(per_unit(10, 3), "3.33");
        assert_eq!(per_unit(11, 3), "3.67");
        assert_eq!(per_unit(i64::MAX, 1), number(i64::MAX));
    }
}
