//! Shares the single bar slot fairly between every registered app.
//!
//! Apps take turns (round robin) so one app with many items can't crowd out the
//! others; within an app, items rotate weighted by `weight`.

use std::collections::BTreeMap;

use crate::model::Item;

#[derive(Debug, Default)]
pub struct Rotator {
    /// app_id -> live items for that app, in display order.
    apps: BTreeMap<String, Vec<Item>>,
    /// app_id -> position in its weighted cycle.
    cursors: BTreeMap<String, usize>,
    last_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub app_id: String,
    pub item: Item,
}

impl Rotator {
    pub fn set_items(&mut self, app_id: &str, items: Vec<Item>) {
        if items.is_empty() {
            self.apps.remove(app_id);
        } else {
            self.apps.insert(app_id.to_string(), items);
        }
    }

    pub fn remove_app(&mut self, app_id: &str) {
        self.apps.remove(app_id);
        self.cursors.remove(app_id);
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Picks what to show next. `now` filters out items outside their window.
    pub fn next(&mut self, now: i64) -> Option<Slot> {
        let ids: Vec<String> = self
            .apps
            .iter()
            .filter(|(_, items)| items.iter().any(|i| i.is_live(now)))
            .map(|(id, _)| id.clone())
            .collect();
        if ids.is_empty() {
            return None;
        }
        let start = match &self.last_app {
            Some(last) => ids.iter().position(|id| id > last).unwrap_or(0),
            None => 0,
        };
        let app_id = ids[start].clone();
        self.last_app = Some(app_id.clone());

        let live: Vec<&Item> = self.apps[&app_id]
            .iter()
            .filter(|i| i.is_live(now))
            .collect();
        let cycle: Vec<&Item> = live
            .iter()
            .flat_map(|i| std::iter::repeat_n(*i, i.weight.max(1) as usize))
            .collect();
        let cursor = self.cursors.entry(app_id.clone()).or_insert(0);
        let item = cycle[*cursor % cycle.len()].clone();
        *cursor = cursor.wrapping_add(1);
        Some(Slot { app_id, item })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, weight: u32) -> Item {
        serde_json::from_value(serde_json::json!({"id": id, "title": id, "weight": weight}))
            .unwrap()
    }

    #[test]
    fn apps_take_turns() {
        let mut r = Rotator::default();
        r.set_items("a", vec![item("a1", 1), item("a2", 1), item("a3", 1)]);
        r.set_items("b", vec![item("b1", 1)]);
        let seq: Vec<String> = (0..4).map(|_| r.next(0).unwrap().item.id).collect();
        assert_eq!(seq, ["a1", "b1", "a2", "b1"]);
    }

    #[test]
    fn weights_repeat_items() {
        let mut r = Rotator::default();
        r.set_items("a", vec![item("x", 2), item("y", 1)]);
        let seq: Vec<String> = (0..3).map(|_| r.next(0).unwrap().item.id).collect();
        assert_eq!(seq, ["x", "x", "y"]);
    }

    #[test]
    fn skips_items_outside_window() {
        let mut r = Rotator::default();
        let mut later = item("later", 1);
        later.starts_at = Some(100);
        r.set_items("a", vec![later]);
        assert!(r.next(50).is_none());
        assert_eq!(r.next(150).unwrap().item.id, "later");
    }
}
