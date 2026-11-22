//! negs-library's Selection with every change made by the core
//! (negs_selection_apply): the selection goes in as JSON and comes back.

use crate::codec;
use negs_core_sys::{library as sys, take_buf};
use oracle::{Item, Json};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    current: Option<Item>,
    selected: HashSet<Item>,
    anchor: Option<Item>,
    lead: Option<Item>,
    base: HashSet<Item>,
}

fn set(s: &HashSet<Item>) -> Json {
    let mut items: Vec<&Item> = s.iter().collect();
    items.sort_by(|a, b| (&a.photo, &a.variant).cmp(&(&b.photo, &b.variant)));
    Json::Array(items.into_iter().map(codec::item).collect())
}

fn opt(i: &Option<Item>) -> Json {
    i.as_ref().map(codec::item).unwrap_or(Json::Null)
}

fn opt_item(v: &Json) -> Option<Item> {
    (!v.is_null()).then(|| codec::to_item(v))
}

fn items(list: &[Item]) -> Json {
    codec::items(list)
}

impl Selection {
    fn to_json(&self) -> Json {
        Json::from_pairs([
            ("current", opt(&self.current)),
            ("selected", set(&self.selected)),
            ("anchor", opt(&self.anchor)),
            ("lead", opt(&self.lead)),
            ("base", set(&self.base)),
        ])
    }

    /// Applies `op` in the core; returns its `items` answer.
    fn apply<const N: usize>(&mut self, pairs: [(&str, Json); N]) -> Json {
        let state = codec::wire(&self.to_json());
        let op = codec::wire(&Json::from_pairs(pairs));
        let bytes = take_buf(unsafe { sys::negs_selection_apply(state.as_ptr(), state.len(), op.as_ptr(), op.len()) });
        assert!(!bytes.is_empty(), "the core refused a selection change: {op}");
        let out = codec::parse(&bytes);
        let s = out.get("selection");
        self.current = opt_item(s.get("current"));
        self.selected = codec::to_items(s.get("selected")).into_iter().collect();
        self.anchor = opt_item(s.get("anchor"));
        self.lead = opt_item(s.get("lead"));
        self.base = codec::to_items(s.get("base")).into_iter().collect();
        out.get("items").clone()
    }

    pub fn current(&self) -> Option<&Item> {
        self.current.as_ref()
    }

    pub fn selected(&self) -> &HashSet<Item> {
        &self.selected
    }

    pub fn anchor(&self) -> Option<&Item> {
        self.anchor.as_ref()
    }

    pub fn lead(&self) -> Option<&Item> {
        self.lead.as_ref()
    }

    pub fn contains(&self, item: &Item) -> bool {
        self.selected.contains(item)
    }

    pub fn select(&mut self, item: &Item) {
        self.apply([("op", Json::string("select")), ("item", codec::item(item))]);
    }

    pub fn select_all_of(&mut self, list: &[Item]) {
        self.apply([("op", Json::string("selectAllOf")), ("items", items(list))]);
    }

    pub fn toggle(&mut self, item: &Item, order: Option<&[Item]>) {
        self.apply([("op", Json::string("toggle")), ("item", codec::item(item)), ("order", order.map(items).unwrap_or(Json::Null))]);
    }

    pub fn extend_to(&mut self, item: &Item, order: &[Item]) {
        self.apply([("op", Json::string("extendTo")), ("item", codec::item(item)), ("order", items(order))]);
    }

    pub fn extend_by(&mut self, offset: isize, order: &[Item]) {
        self.apply([("op", Json::string("extendBy")), ("offset", Json::Number(offset as f64)), ("order", items(order))]);
    }

    pub fn select_all(&mut self, shown: &[Item]) {
        self.apply([("op", Json::string("selectAll")), ("shown", items(shown))]);
    }

    pub fn deselect_all(&mut self) {
        self.apply([("op", Json::string("deselectAll"))]);
    }

    pub fn band(&mut self, list: &[Item], base: &HashSet<Item>) {
        self.apply([("op", Json::string("band")), ("items", items(list)), ("base", set(base))]);
    }

    pub fn prune(&mut self, shown: &HashSet<Item>) {
        self.apply([("op", Json::string("prune")), ("shown", set(shown))]);
    }

    pub fn rename(&mut self, from: &Item, to: &Item) {
        self.apply([("op", Json::string("rename")), ("from", codec::item(from)), ("to", codec::item(to))]);
    }

    pub fn ordered(&self, order: &[Item]) -> Vec<Item> {
        let mut copy = self.clone();
        codec::to_items(&copy.apply([("op", Json::string("ordered")), ("order", items(order))]))
    }
}

/// What to show after `item` and those in `gone` go.
pub fn next_after(item: &Item, order: &[Item], gone: &HashSet<Item>) -> Option<Item> {
    let mut s = Selection::default();
    let answer = s.apply([("op", Json::string("nextAfter")), ("item", codec::item(item)), ("order", items(order)), ("gone", set(gone))]);
    codec::to_items(&answer).into_iter().next()
}
