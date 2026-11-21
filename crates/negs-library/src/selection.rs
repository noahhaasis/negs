//! The selected photographs and the current one (`LibrarySelection` in
//! `Selection.swift`). A plain click selects one and makes it current;
//! modifier clicks change only the selection. The current one is always
//! selected.

use crate::item::Item;
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    current: Option<Item>,
    selected: HashSet<Item>,
    /// Where a range starts (the last plain or Ctrl-click) and where it
    /// ends now (the last Shift-click or Shift-arrow).
    anchor: Option<Item>,
    lead: Option<Item>,
    /// The selection a range is added to: as it was when the anchor was set.
    base: HashSet<Item>,
}

impl Selection {
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

    /// Plain click: only `item`, and it's current.
    pub fn select(&mut self, item: &Item) {
        self.current = Some(item.clone());
        self.selected = HashSet::from([item.clone()]);
        self.anchor = Some(item.clone());
        self.lead = Some(item.clone());
        self.base = self.selected.clone();
    }

    /// `items` alone, the first current: new duplicates.
    pub fn select_all_of(&mut self, items: &[Item]) {
        let Some(first) = items.first() else { return };
        self.select(first);
        self.selected = items.iter().cloned().collect();
        self.base = self.selected.clone();
    }

    /// Ctrl-click: adds or removes `item`. The current one stays, unless
    /// it's the one removed and `order` is given (the grid): then the next
    /// selected one to its right becomes current, or the nearest to its
    /// left. The only one selected stays.
    pub fn toggle(&mut self, item: &Item, order: Option<&[Item]>) {
        let Some(current) = self.current.clone() else { return self.select(item) };
        if *item == current {
            let Some(order) = order else { return };
            let Some(index) = order.iter().position(|i| *i == current) else { return };
            let next = order[index + 1..].iter().find(|i| self.selected.contains(*i)).or_else(|| order[..index].iter().rev().find(|i| self.selected.contains(*i)));
            let Some(next) = next.cloned() else { return };
            self.selected.remove(&current);
            self.current = Some(next.clone());
            self.anchor = Some(next.clone());
            self.lead = Some(next);
            self.base = self.selected.clone();
            return;
        }
        if !self.selected.remove(item) {
            self.selected.insert(item.clone());
        }
        self.anchor = Some(item.clone());
        self.lead = Some(item.clone());
        self.base = self.selected.clone();
    }

    /// Shift-click or Shift-arrow: the items from the anchor to `item`, in
    /// `order`, added to the selection as it was when the anchor was set.
    pub fn extend_to(&mut self, item: &Item, order: &[Item]) {
        let Some(current) = self.current.clone() else { return self.select(item) };
        let start = self.anchor.as_ref().and_then(|a| order.iter().position(|i| i == a)).or_else(|| order.iter().position(|i| *i == current));
        let (Some(start), Some(end)) = (start, order.iter().position(|i| i == item)) else { return };
        let mut selected = self.base.clone();
        selected.extend(order[start.min(end)..=start.max(end)].iter().cloned());
        selected.insert(current);
        self.selected = selected;
        self.lead = Some(item.clone());
    }

    /// Shift-arrow: moves the range's end `offset` places on in `order`.
    pub fn extend_by(&mut self, offset: isize, order: &[Item]) {
        let from = self.lead.as_ref().or(self.current.as_ref()).and_then(|f| order.iter().position(|i| i == f));
        let Some(from) = from else { return };
        let to = (from as isize + offset).clamp(0, order.len() as isize - 1) as usize;
        let target = order[to].clone();
        self.extend_to(&target, order);
    }

    /// Ctrl+A: everything shown.
    pub fn select_all(&mut self, shown: &[Item]) {
        let Some(current) = self.current.clone() else { return };
        self.selected = shown.iter().cloned().collect();
        self.selected.insert(current.clone());
        self.anchor = Some(current.clone());
        self.lead = Some(current);
        self.base = self.selected.clone();
    }

    /// All but the current one.
    pub fn deselect_all(&mut self) {
        if let Some(current) = self.current.clone() {
            self.select(&current);
        }
    }

    /// Rubber band: `base` and the items inside the band.
    pub fn band(&mut self, items: &[Item], base: &HashSet<Item>) {
        let Some(current) = self.current.clone() else { return };
        let mut selected = base.clone();
        selected.extend(items.iter().cloned());
        selected.insert(current.clone());
        self.selected = selected;
        self.lead = Some(current);
        self.base = self.selected.clone();
    }

    /// Keeps only the items in `shown`, such as after the filter changed.
    pub fn prune(&mut self, shown: &HashSet<Item>) {
        let current = self.current.clone();
        let pruned: HashSet<Item> = self.selected.iter().filter(|i| shown.contains(*i) || Some(*i) == current.as_ref()).cloned().collect();
        if pruned != self.selected {
            self.base = self.base.intersection(&pruned).cloned().collect();
            self.selected = pruned;
        }
        if self.anchor.as_ref().is_some_and(|a| !self.selected.contains(a)) {
            self.anchor = current.clone();
        }
        if self.lead.as_ref().is_some_and(|l| !self.selected.contains(l)) {
            self.lead = current;
        }
    }

    /// `from` is now called `to`: a duplicate that took its photograph's
    /// place when the photograph was deleted.
    pub fn rename(&mut self, from: &Item, to: &Item) {
        let swap = |i: &mut Option<Item>| {
            if i.as_ref() == Some(from) {
                *i = Some(to.clone());
            }
        };
        swap(&mut self.current);
        swap(&mut self.anchor);
        swap(&mut self.lead);
        for set in [&mut self.selected, &mut self.base] {
            if set.remove(from) {
                set.insert(to.clone());
            }
        }
    }

    /// The selected items in `order`.
    pub fn ordered(&self, order: &[Item]) -> Vec<Item> {
        order.iter().filter(|i| self.selected.contains(*i)).cloned().collect()
    }
}

/// The first item after `item` in `order` that isn't `gone`, or else the
/// last one before it (`EditorModel.next`): what becomes current when
/// `item` leaves the strip.
pub fn next_after(item: &Item, order: &[Item], gone: &HashSet<Item>) -> Option<Item> {
    let Some(index) = order.iter().position(|i| i == item) else { return order.iter().find(|i| !gone.contains(*i)).cloned() };
    order[index..].iter().find(|i| !gone.contains(*i)).or_else(|| order[..index].iter().rev().find(|i| !gone.contains(*i))).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(names: &[&str]) -> Vec<Item> {
        names.iter().map(|n| Item::photo(*n)).collect()
    }

    #[test]
    fn clicks_and_ranges() {
        let order = items(&["a", "b", "c", "d", "e"]);
        let mut s = Selection::default();
        s.select(&order[1]);
        s.toggle(&order[3], Some(&order));
        assert_eq!(s.ordered(&order), items(&["b", "d"]));
        s.extend_to(&order[4], &order);
        assert_eq!(s.ordered(&order), items(&["b", "d", "e"]), "from the anchor, added to the base");
        s.extend_by(-2, &order);
        assert_eq!(s.ordered(&order), items(&["b", "c", "d"]));
        // Ctrl-clicking the current one hands current on, to the right first.
        s.toggle(&order[1], Some(&order));
        assert_eq!(s.current(), Some(&order[2]));
        assert!(!s.contains(&order[1]));
        s.select_all(&order);
        assert_eq!(s.selected().len(), 5);
        s.prune(&items(&["a", "b"]).into_iter().collect());
        assert_eq!(s.ordered(&order), items(&["a", "b", "c"]), "the current one stays");
        s.deselect_all();
        assert_eq!(s.ordered(&order), items(&["c"]));
    }

    #[test]
    fn the_next_one_after_a_gone_one() {
        let order = items(&["a", "b", "c"]);
        let gone: HashSet<Item> = items(&["b", "c"]).into_iter().collect();
        assert_eq!(next_after(&order[1], &order, &gone), Some(order[0].clone()));
        let gone: HashSet<Item> = items(&["b"]).into_iter().collect();
        assert_eq!(next_after(&order[1], &order, &gone), Some(order[2].clone()));
    }
}
