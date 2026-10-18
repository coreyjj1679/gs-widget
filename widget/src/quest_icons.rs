//! Prepare quest item and monster strips without decoding images on the UI thread.
use std::collections::HashSet;

use slint::{ModelRc, VecModel};

use super::{db, IconJob, QuestItemChip, Session};

/// Share `pending` across every strip in a publication, and publish the models
/// before sending `jobs`. A namespaced item or monster key paints its occurrences
/// in both views without confusing entities that happen to share a numeric code.
pub(crate) fn item_chips(
    catalog: &db::Catalog,
    session: &Session,
    items: &[db::QuestItemRef],
    gen: u64,
    links: bool,
    pending: &mut HashSet<String>,
    jobs: &mut Vec<IconJob>,
) -> ModelRc<QuestItemChip> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|item| {
                let icon = catalog.linked_icon(&item.key).and_then(|(path, frame)| {
                    let cache_key = format!("{}#{frame}", path.display());
                    let cached = session.cache.borrow().get(&cache_key).cloned();
                    if cached.is_none() && pending.insert(item.key.clone()) {
                        jobs.push(IconJob {
                            gen,
                            links,
                            cache_key,
                            row_key: format!("#quest-item:{}", item.key),
                            path,
                            frame,
                        });
                    }
                    cached
                });
                QuestItemChip {
                    key: item.key.clone().into(),
                    title: item.primary.clone().into(),
                    alternate: item.alternate.clone().into(),
                    quantity: item.quantity.clone().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>(),
    ))
}
