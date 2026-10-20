//! A separate retained draft for character acquisition costs.
use super::*;
use crate::{CharacterCalcChoice, CharacterCalcMaterial};

#[cfg(test)]
#[path = "characters_tests.rs"]
mod tests;

struct CharacterController {
    catalog: Rc<db::Catalog>,
    cache: IconCache,
    document: RefCell<Option<db::CharacterCalcDocument>>,
    routes: RefCell<Vec<String>>,
    errors: RefCell<InputErrors>,
    load_error: Option<String>,
}

fn text<'a>(lang: &str, zh: &'a str, en: &'a str, ko: &'a str) -> &'a str {
    if db::primary_lang(lang) == "ko" {
        ko
    } else {
        tr(lang, zh, en)
    }
}

impl CharacterController {
    fn image(&self, source: Option<(std::path::PathBuf, i32)>) -> (Image, bool) {
        let image = source.and_then(|(path, frame)| icons::cached_icon(&self.cache, &path, frame));
        let present = image.is_some();
        (image.unwrap_or_default(), present)
    }

    fn search(&self, app: &App, query: &str) {
        let rows = self
            .catalog
            .character_calculator_candidates(query, &app.get_lang())
            .into_iter()
            .map(|candidate| {
                let (icon, has_icon) = self.image(candidate.icon);
                CharacterCalcChoice {
                    id: candidate.code.into(),
                    title: candidate.title.into(),
                    subtitle: format!(
                        "{} · {} {}",
                        candidate.subtitle,
                        candidate.route_count,
                        text(&app.get_lang(), "條路線", "routes", "개 경로")
                    )
                    .into(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_character_calculator_candidates(ModelRc::new(VecModel::from(rows)));
    }

    fn publish_errors(&self, app: &App) {
        let errors = self.errors.borrow();
        let error = self
            .load_error
            .as_deref()
            .or_else(|| errors.first())
            .unwrap_or_default();
        app.set_character_calculator_error(db::localized_error(&app.get_lang(), error).into());
        let draft = errors.draft(&InputField::Quantity);
        app.set_character_calculator_quantity_draft(draft.unwrap_or_default().into());
        app.set_character_calculator_quantity_has_draft(draft.is_some());
        if !error.is_empty() {
            app.set_character_calculator_complete(false);
            app.set_character_calculator_unit_total("—".into());
        }
        let rows = app.get_character_calculator_materials();
        for index in 0..rows.row_count() {
            let Some(mut row) = rows.row_data(index) else {
                continue;
            };
            let draft = errors.draft(&InputField::Price(row.key.to_string()));
            if row.has_draft != draft.is_some() || row.draft.as_str() != draft.unwrap_or_default() {
                row.draft = draft.unwrap_or_default().into();
                row.has_draft = draft.is_some();
                rows.set_row_data(index, row);
            }
        }
    }

    fn render(&self, app: &App) {
        self.publish_errors(app);
        let lang = app.get_lang();
        let document = self.document.borrow();
        let Some(document) = document.as_ref() else {
            app.set_character_calculator_selected_name("".into());
            app.set_character_calculator_materials(ModelRc::default());
            app.set_character_calculator_routes(ModelRc::default());
            return;
        };
        let doc = self
            .catalog
            .localized_character_calculator_document(document, &lang);
        let costs = match doc.calculate() {
            Ok(costs) => costs,
            Err(error) => {
                app.set_character_calculator_error(db::localized_error(&lang, &error).into());
                return;
            }
        };
        app.set_character_calculator_selected_name(doc.title.clone().into());
        let (icon, has_icon) = self.image(
            self.catalog
                .character_calculator_icon(&format!("hero:{}", doc.hero_code)),
        );
        app.set_character_calculator_selected_icon(icon);
        app.set_character_calculator_selected_has_icon(has_icon);
        app.set_character_calculator_quantity(doc.quantity.to_string().into());
        app.set_character_calculator_conditions(doc.conditions.clone().into());
        let routes = self
            .catalog
            .character_calculator_routes(&doc.hero_code, &lang);
        let mut ids = vec![doc.route_id.clone()];
        let mut labels = vec![doc.route_title.clone().into()];
        for route in routes {
            if route.id != doc.route_id {
                ids.push(route.id);
                labels.push(route.title.into());
            }
        }
        *self.routes.borrow_mut() = ids;
        app.set_character_calculator_routes(ModelRc::new(VecModel::from(labels)));
        app.set_character_calculator_route_index(0);
        let rows = doc
            .inputs
            .iter()
            .zip(&costs.inputs)
            .map(|(input, cost)| {
                let (icon, has_icon) =
                    self.image(self.catalog.character_calculator_icon(&input.key));
                let subtitle = match input.kind.as_str() {
                    "hero" => text(
                        &lang,
                        "角色取得總成本；已持有可填 0",
                        "Full acquisition cost; enter 0 if already owned",
                        "캐릭터 전체 획득 비용 · 보유 시 0",
                    ),
                    "fee" => text(
                        &lang,
                        "每次合成費用；未知請留空",
                        "Fee per synthesis; leave blank if unknown",
                        "1회 합성 비용 · 모르면 비워 두세요",
                    ),
                    "choice" => text(
                        &lang,
                        "依路線條件填入所選材料的合計成本",
                        "Combined cost of the required choice of materials",
                        "선택한 재료의 합계 비용",
                    ),
                    _ => text(&lang, "道具單價", "Item unit price", "아이템 단가"),
                };
                let errors = self.errors.borrow();
                let draft = errors.draft(&InputField::Price(input.key.clone()));
                CharacterCalcMaterial {
                    key: input.key.clone().into(),
                    title: input.title.clone().into(),
                    subtitle: subtitle.into(),
                    quantity: number(cost.quantity).into(),
                    price: cost.unit_price.map(number).unwrap_or_default().into(),
                    draft: draft.unwrap_or_default().into(),
                    has_draft: draft.is_some(),
                    subtotal: cost
                        .line_total
                        .map(number)
                        .unwrap_or_else(|| "—".into())
                        .into(),
                    missing_price: cost.line_total.is_none(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_character_calculator_materials(update_rows(
            app.get_character_calculator_materials(),
            rows,
        ));
        app.set_character_calculator_material_total(number(costs.material_total).into());
        app.set_character_calculator_fee(
            if doc
                .inputs
                .iter()
                .any(|input| input.kind == "fee" && input.unit_price.is_none())
            {
                "—".into()
            } else {
                number(costs.fee).into()
            },
        );
        app.set_character_calculator_total(number(costs.known_total).into());
        app.set_character_calculator_unit_total(
            costs
                .total
                .map(|total| per_unit(total, doc.quantity))
                .unwrap_or_else(|| "—".into())
                .into(),
        );
        app.set_character_calculator_complete(costs.total.is_some());
        app.set_character_calculator_status(
            if costs.total.is_some() {
                text(
                    &lang,
                    "所有成本已填寫 · 單位：兩",
                    "All costs entered · Currency: coins",
                    "모든 비용 입력 완료 · 단위: 냥",
                )
                .to_string()
            } else {
                format!(
                    "{} {} · {}",
                    doc.inputs.len() - costs.priced,
                    text(
                        &lang,
                        "項成本尚未填寫",
                        "costs still unknown",
                        "개 비용 미입력"
                    ),
                    text(
                        &lang,
                        "0 代表已持有或免費，留空代表未知",
                        "0 means owned or free; blank means unknown",
                        "보유 또는 무료는 0, 모르면 빈칸"
                    )
                )
            }
            .into(),
        );
        app.set_character_calculator_notice(text(&lang,
            "計算所選路線的直接成本；前置角色請填取得總成本，未自動展開其轉職鏈。等級、任務等條件不換算為金額。單價由用家輸入。",
            "Direct costs for the selected route. Enter each prerequisite character’s full acquisition cost; earlier promotion chains are not expanded automatically. Level and quest requirements are not priced. Prices are your estimates.",
            "선택한 경로의 직접 비용입니다. 선행 캐릭터의 전체 획득 비용을 입력하세요. 이전 전직 경로는 자동으로 계산하지 않으며, 레벨·퀘스트 조건은 금액으로 환산하지 않습니다.").into());
        self.publish_errors(app);
    }

    fn persist(&self, app: &App, next: db::CharacterCalcDocument) -> Result<(), String> {
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        if self.document.borrow().as_ref() != Some(&next) {
            next.calculate()?;
            db::save_character_calculator(&next)?;
            *self.document.borrow_mut() = Some(next);
        }
        self.render(app);
        Ok(())
    }

    fn select(&self, app: &App, code: &str, route: &str, preserve_quantity: bool) {
        let result = self
            .catalog
            .character_calculator_document(code, route, &app.get_lang())
            .and_then(|mut next| {
                if let Some(old) = self.document.borrow().as_ref() {
                    if preserve_quantity {
                        next.quantity = old.quantity;
                    }
                    for input in &mut next.inputs {
                        // Fixed, catalogue-provided costs stay with their route.
                        let reusable = matches!(input.kind.as_str(), "hero" | "item")
                            || (old.hero_code == next.hero_code && old.route_id == next.route_id);
                        if reusable && input.unit_price.is_none() {
                            if let Some(prior) =
                                old.inputs.iter().find(|prior| prior.key == input.key)
                            {
                                input.unit_price = prior.unit_price;
                            }
                        }
                    }
                }
                self.persist(app, next)
            });
        match result {
            Ok(()) => {
                self.errors.borrow_mut().clear();
                self.render(app);
                app.set_character_calculator_input_revision(
                    app.get_character_calculator_input_revision()
                        .wrapping_add(1),
                );
            }
            Err(error) => app.set_character_calculator_error(
                db::localized_error(&app.get_lang(), &error).into(),
            ),
        }
    }

    fn edit(&self, app: &App, field: InputField, raw: &str) -> bool {
        let current = self.document.borrow().clone();
        let result = current
            .ok_or_else(|| {
                text(
                    &app.get_lang(),
                    "請先選擇角色。",
                    "Choose a character first.",
                    "캐릭터를 먼저 선택하세요.",
                )
                .to_string()
            })
            .and_then(|mut doc| {
                let value = parse_amount(raw, &app.get_lang())?;
                match &field {
                    InputField::Quantity => {
                        doc.quantity = value.filter(|quantity| *quantity > 0).ok_or_else(|| {
                            text(
                                &app.get_lang(),
                                "製作數量必須是大於 0 的整數。",
                                "Crafting quantity must be a positive whole number.",
                                "합성 수량은 0보다 큰 정수여야 합니다.",
                            )
                            .to_string()
                        })?;
                    }
                    InputField::Price(key) => {
                        let row = doc
                            .inputs
                            .iter_mut()
                            .find(|input| input.key == *key)
                            .ok_or_else(|| "找不到成本項目 / Cost entry not found".to_string())?;
                        row.unit_price = value;
                    }
                }
                // Drop only this field's old invalid draft before rendering the
                // accepted value; all other visible invalid drafts survive.
                self.errors.borrow_mut().0.remove(&field);
                self.persist(app, doc)
            });
        let valid = self.errors.borrow_mut().record(field, raw, result);
        self.publish_errors(app);
        valid
    }
}

pub(super) fn install(app: &App, catalog: Rc<db::Catalog>, cache: IconCache) {
    let (document, load_error) = match db::load_character_calculator() {
        Ok(Some(document)) => match catalog.supplement_character_calculator_materials(&document) {
            Ok(updated) if updated != document => {
                let error = db::save_character_calculator(&updated).err();
                (Some(updated), error)
            }
            Ok(updated) => (Some(updated), None),
            Err(error) => (Some(document), Some(error)),
        },
        Ok(None) => (None, None),
        Err(error) => (None, Some(error)),
    };
    let controller = Rc::new(CharacterController {
        catalog,
        cache,
        document: RefCell::new(document),
        routes: RefCell::default(),
        errors: RefCell::default(),
        load_error,
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_refresh(move || {
        if let Some(app) = weak.upgrade() {
            c.render(&app);
            c.search(&app, "");
        }
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_search(move |query| {
        if let Some(app) = weak.upgrade() {
            c.search(&app, &query);
        }
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_select_character(move |code| {
        let Some(app) = weak.upgrade() else { return };
        if let Some(route) = c
            .catalog
            .character_calculator_routes(&code, &app.get_lang())
            .first()
        {
            c.select(&app, &code, &route.id, false);
        }
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_select_route(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let route = usize::try_from(index)
            .ok()
            .and_then(|index| c.routes.borrow().get(index).cloned());
        let document = c.document.borrow().clone();
        if let (Some(route), Some(doc)) = (route, document) {
            if route != doc.route_id {
                c.select(&app, &doc.hero_code, &route, true);
            }
        }
        app.set_character_calculator_route_index(0);
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_quantity_changed(move |raw| {
        weak.upgrade()
            .is_some_and(|app| c.edit(&app, InputField::Quantity, &raw))
    });
    let c = Rc::clone(&controller);
    let weak = app.as_weak();
    app.on_character_calculator_price_changed(move |key, raw| {
        weak.upgrade()
            .is_some_and(|app| c.edit(&app, InputField::Price(key.to_string()), &raw))
    });
    controller.render(app);
    controller.search(app, "");
}
