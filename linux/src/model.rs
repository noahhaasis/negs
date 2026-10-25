//! Local presentation/interaction state only. No film math or engine defaults live here.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub values: BTreeMap<String, f64>,
    pub choices: BTreeMap<String, String>,
    pub rotation: u8,
    pub border: u8,
    pub crop: Option<[f64; 4]>,
}

#[derive(Default)]
pub struct History {
    pub state: State,
    undo: Vec<(String, State)>,
    redo: Vec<(String, State)>,
    pending: Option<(String, State)>,
}

impl History {
    pub fn new(state: State) -> Self {
        Self {
            state,
            ..Default::default()
        }
    }
    pub fn begin(&mut self, name: &str) {
        if self.pending.is_none() {
            self.pending = Some((name.into(), self.state.clone()));
        }
    }

    pub fn commit(&mut self) {
        if let Some((name, before)) = self.pending.take()
            && before != self.state
        {
            self.undo.push((name, before));
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn undo(&mut self) -> Option<String> {
        self.commit();
        let (name, state) = self.undo.pop()?;
        self.redo
            .push((name.clone(), std::mem::replace(&mut self.state, state)));
        Some(name)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.commit();
        let (name, state) = self.redo.pop()?;
        self.undo
            .push((name.clone(), std::mem::replace(&mut self.state, state)));
        Some(name)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_drag_is_one_undo_step_and_restores_exact_value() {
        let mut h = History::default();
        h.state.values.insert("exposure".into(), 0.3);
        h.begin("Exposure");
        for i in 0..240 {
            h.begin("Exposure");
            h.state.values.insert("exposure".into(), i as f64 * 0.1);
        }
        h.commit();
        assert_eq!(h.undo().as_deref(), Some("Exposure"));
        assert_eq!(h.state.values["exposure"], 0.3);
        assert!(h.undo().is_none());
        h.redo();
        assert_eq!(h.state.values["exposure"], 23.900000000000002);
    }

    #[test]
    fn abandoned_redo_and_noop_gestures() {
        let mut h = History::default();
        h.begin("No change");
        h.commit();
        assert!(!h.can_undo());
        h.begin("Rotate");
        h.state.rotation = 1;
        h.commit();
        h.undo();
        h.begin("Border");
        h.state.border = 2;
        h.commit();
        assert!(h.redo().is_none());
        assert_eq!(h.state.rotation, 0);
        assert_eq!(h.state.border, 2);
    }
}
