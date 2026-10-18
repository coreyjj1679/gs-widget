//! Explicitly saved sheets are independent from the automatically retained draft.
use super::*;

impl Controller {
    pub(super) fn render_library(&self, app: &App) {
        let english = app.get_lang() == "en-zh";
        let library = self.library.borrow();
        app.set_calculator_active_saved_name("".into());
        let Ok(book) = library.as_ref() else {
            app.set_calculator_library_error(library.as_ref().unwrap_err().clone().into());
            app.set_calculator_folders(ModelRc::default());
            app.set_calculator_saved(ModelRc::default());
            app.set_calculator_saved_count(0);
            app.set_calculator_save_state("".into());
            return;
        };
        app.set_calculator_library_error("".into());
        let index = book
            .folders
            .iter()
            .position(|folder| folder.id == self.folder_id.get())
            .unwrap_or(0);
        let folder = &book.folders[index];
        self.folder_id.set(folder.id);
        app.set_calculator_folders(ModelRc::new(VecModel::from(
            book.folders
                .iter()
                .map(|folder| folder.name.clone().into())
                .collect::<Vec<_>>(),
        )));
        app.set_calculator_folder_index(index as i32);
        app.set_calculator_saved_count(
            book.folders
                .iter()
                .map(|folder| folder.entries.len())
                .sum::<usize>() as i32,
        );
        let rows = folder
            .entries
            .iter()
            .map(|entry| {
                let (icon, has_icon) = self.icon(entry.document.item_code);
                let cost = entry
                    .document
                    .calculate()
                    .expect("validated saved calculation");
                CalcSaved {
                    id: entry.id.to_string().into(),
                    title: entry.name.clone().into(),
                    subtitle: format!(
                        "{} · {} {} · {} {}",
                        entry.document.title,
                        tr(english, "產出", "output"),
                        number(cost.output_units),
                        tr(
                            english,
                            if cost.total.is_some() {
                                "成本"
                            } else {
                                "已知成本"
                            },
                            if cost.total.is_some() {
                                "cost"
                            } else {
                                "known cost"
                            }
                        ),
                        number(cost.known_total)
                    )
                    .into(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_calculator_saved(update_rows(app.get_calculator_saved(), rows));
        let current = self.document.borrow();
        let active = self.active_entry.get().and_then(|id| {
            book.folders.iter().find_map(|folder| {
                folder
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| (folder, entry))
            })
        });
        if active.is_none() {
            self.active_entry.set(None);
        }
        let state = match active {
            Some((folder, entry)) => {
                app.set_calculator_active_saved_name(entry.name.clone().into());
                if current.as_ref() == Some(&entry.document) {
                    format!("{} · {}", tr(english, "已儲存", "Saved"), folder.name)
                } else {
                    tr(english, "有未儲存變更", "Unsaved changes").into()
                }
            }
            None if current.is_some() => {
                tr(english, "尚未加入收藏", "Not yet saved to your collection").into()
            }
            None => String::new(),
        };
        app.set_calculator_save_state(state.into());
    }

    fn folder_at(&self, index: i32) -> Result<u64, String> {
        let library = self.library.borrow();
        let book = library.as_ref().map_err(Clone::clone)?;
        usize::try_from(index)
            .ok()
            .and_then(|index| book.folders.get(index))
            .map(|folder| folder.id)
            .ok_or_else(|| "找不到資料夾 / Folder not found".into())
    }

    // Publish a mutation only after its atomic file replacement succeeds. All
    // failures leave both the current collection and selected IDs unchanged.
    fn change_library<T>(
        &self,
        app: &App,
        operation: impl FnOnce(&mut db::CalcLibrary) -> Result<T, String>,
    ) -> Option<T> {
        let mut next = match self.library.borrow().as_ref() {
            Ok(book) => book.clone(),
            Err(error) => {
                app.set_calculator_error(error.clone().into());
                return None;
            }
        };
        match db::load_calculator_library() {
            Ok(latest) if latest != next => {
                *self.library.borrow_mut() = Ok(latest);
                self.render_library(app);
                app.set_calculator_error(tr(app.get_lang() == "en-zh",
                    "收藏已在其他視窗更新，已重新載入。請檢查後重試操作。",
                    "The collection changed in another window and was reloaded. Review it and retry.").into());
                return None;
            }
            Err(error) => {
                app.set_calculator_error(error.into());
                return None;
            }
            _ => (),
        }
        let result = match operation(&mut next)
            .and_then(|result| db::save_calculator_library(&next).map(|_| result))
        {
            Ok(result) => result,
            Err(error) => {
                app.set_calculator_error(error.into());
                return None;
            }
        };
        *self.library.borrow_mut() = Ok(next);
        app.set_calculator_error("".into());
        app.set_calculator_info("".into());
        Some(result)
    }

    pub(super) fn import_folder(&self, app: &App, code: &str) {
        let share = match db::decode_calculator_folder(code) {
            Ok(share) => share,
            Err(error) => {
                app.set_calculator_error(error.into());
                return;
            }
        };
        let count = share.entries.len();
        if let Some(id) = self.change_library(app, |book| book.import_folder(share)) {
            self.folder_id.set(id);
            self.render_library(app);
            app.set_calculator_library_open(true);
            app.set_calculator_info(
                format!(
                    "{} {count} {}",
                    tr(
                        app.get_lang() == "en-zh",
                        "已匯入新資料夾，包含",
                        "Imported a new folder containing"
                    ),
                    tr(
                        app.get_lang() == "en-zh",
                        "張成本單；原有收藏已保留。",
                        "cost sheets; existing collections were preserved."
                    )
                )
                .into(),
            );
        }
    }
}

pub(super) fn install_callbacks(app: &App, controller: &Rc<Controller>) {
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_select_folder(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if let Ok(id) = c.folder_at(index) {
            c.folder_id.set(id);
        }
        c.render_library(&app);
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_new_folder(move |name| {
        let Some(app) = weak.upgrade() else { return };
        if let Some(id) = c.change_library(&app, |book| book.new_folder(&name)) {
            c.folder_id.set(id);
            c.render_library(&app);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_rename_folder(move |name| {
        let Some(app) = weak.upgrade() else { return };
        if c.change_library(&app, |book| book.rename_folder(c.folder_id.get(), &name))
            .is_some()
        {
            c.render_library(&app);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_delete_folder(move || {
        let Some(app) = weak.upgrade() else { return };
        if c.change_library(&app, |book| book.remove_folder(c.folder_id.get()))
            .is_some()
        {
            c.render_library(&app);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_save_sheet(move |name, index, as_copy| {
        let Some(app) = weak.upgrade() else { return };
        let Some(doc) = c.document.borrow().clone() else {
            return;
        };
        let folder_id = match c.folder_at(index) {
            Ok(id) => id,
            Err(error) => {
                app.set_calculator_error(error.into());
                return;
            }
        };
        let entry_id = if as_copy { None } else { c.active_entry.get() };
        if let Some(id) = c.change_library(&app, |book| {
            book.save_entry(folder_id, entry_id, &name, doc)
        }) {
            c.active_entry.set(Some(id));
            c.folder_id.set(folder_id);
            c.render_library(&app);
            app.set_calculator_info(
                tr(
                    app.get_lang() == "en-zh",
                    "成本單已儲存到收藏。",
                    "Cost sheet saved to your collection.",
                )
                .into(),
            );
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_open_saved(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(id) = id.parse::<u64>() else { return };
        let doc = c.library.borrow().as_ref().ok().and_then(|book| {
            book.folders
                .iter()
                .flat_map(|folder| &folder.entries)
                .find(|entry| entry.id == id)
                .map(|entry| entry.document.clone())
        });
        if let Some(doc) = doc {
            if c.commit(&app, doc, "") {
                c.active_entry.set(Some(id));
                c.render_library(&app);
                app.set_calculator_library_open(false);
            }
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_delete_saved(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(id) = id.parse::<u64>() else { return };
        if c.change_library(&app, |book| book.remove_entry(id))
            .is_some()
        {
            if c.active_entry.get() == Some(id) {
                c.active_entry.set(None);
            }
            c.render_library(&app);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_move_saved(move |id, index| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(id) = id.parse::<u64>() else { return };
        let folder = match c.folder_at(index) {
            Ok(folder) => folder,
            Err(error) => {
                app.set_calculator_error(error.into());
                return;
            }
        };
        if c.change_library(&app, |book| book.move_entry(id, folder))
            .is_some()
        {
            c.render_library(&app);
        }
    });
    let weak = app.as_weak();
    let c = Rc::clone(controller);
    app.on_calculator_export_folder(move || {
        let Some(app) = weak.upgrade() else { return };
        let library = c.library.borrow();
        let Ok(book) = library.as_ref() else { return };
        let Some(folder) = book
            .folders
            .iter()
            .find(|folder| folder.id == c.folder_id.get())
        else {
            return;
        };
        match db::encode_calculator_folder(folder) {
            Ok(code) => {
                window::copy_to_clipboard(&code);
                app.set_calculator_error("".into());
                app.set_calculator_info(
                    format!(
                        "{} {} {}",
                        tr(
                            app.get_lang() == "en-zh",
                            "資料夾分享碼已複製，包含",
                            "Folder code copied, containing"
                        ),
                        folder.entries.len(),
                        tr(
                            app.get_lang() == "en-zh",
                            "張成本單。對方可離線匯入整個資料夾。",
                            "cost sheets. The entire folder can be imported offline."
                        )
                    )
                    .into(),
                );
            }
            Err(error) => app.set_calculator_error(error.into()),
        }
    });
}
