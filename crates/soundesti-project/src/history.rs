use crate::Project;

/// Snapshot-based undo history.
///
/// Callers record the state *before* a logical edit (once per gesture, not per
/// mouse-move), so one drag is one undo step. Projects hold no audio data yet, so
/// whole-project snapshots are cheap; this will move to diffs when audio clips land.
///
/// FL Studio semantics (manual, Global Shortcuts):
/// - Ctrl+Z toggles: undo the last edit, or redo it if the last action was that undo.
/// - Ctrl+Alt+Z steps back through history.
pub struct History {
    undo: Vec<(String, Project)>,
    redo: Vec<(String, Project)>,
    limit: usize,
    last_was_undo: bool,
}

impl Default for History {
    fn default() -> Self {
        Self::new(200)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1), last_was_undo: false }
    }

    /// Records `before` as the state to return to when this edit is undone.
    pub fn record(&mut self, before: &Project, label: impl Into<String>) {
        self.undo.push((label.into(), before.clone()));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_was_undo = false;
    }

    /// Step back one edit. Returns the undone edit's label.
    pub fn undo(&mut self, current: &mut Project) -> Option<String> {
        let (label, prev) = self.undo.pop()?;
        let now = std::mem::replace(current, prev);
        self.redo.push((label.clone(), now));
        self.last_was_undo = true;
        Some(label)
    }

    pub fn redo(&mut self, current: &mut Project) -> Option<String> {
        let (label, next) = self.redo.pop()?;
        let now = std::mem::replace(current, next);
        self.undo.push((label.clone(), now));
        self.last_was_undo = false;
        Some(label)
    }

    /// FL's Ctrl+Z: undo, or redo if the previous action was an undo.
    pub fn toggle(&mut self, current: &mut Project) -> Option<String> {
        if self.last_was_undo && !self.redo.is_empty() { self.redo(current) } else { self.undo(current) }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_was_undo = false;
    }

    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().rev().map(|(l, _)| l.as_str())
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

    fn edit(h: &mut History, p: &mut Project, bpm: f64) {
        h.record(p, format!("bpm {bpm}"));
        p.bpm = bpm;
    }

    #[test]
    fn undo_redo_basic() {
        let mut h = History::default();
        let mut p = Project::new();
        edit(&mut h, &mut p, 100.0);
        edit(&mut h, &mut p, 110.0);
        assert_eq!(h.undo(&mut p).as_deref(), Some("bpm 110"));
        assert_eq!(p.bpm, 100.0);
        assert_eq!(h.undo(&mut p).as_deref(), Some("bpm 100"));
        assert_eq!(p.bpm, 130.0);
        assert!(h.undo(&mut p).is_none());
        h.redo(&mut p);
        assert_eq!(p.bpm, 100.0);
    }

    #[test]
    fn ctrl_z_toggles_between_last_two_states() {
        let mut h = History::default();
        let mut p = Project::new();
        edit(&mut h, &mut p, 100.0);
        edit(&mut h, &mut p, 110.0);
        h.toggle(&mut p);
        assert_eq!(p.bpm, 100.0);
        h.toggle(&mut p);
        assert_eq!(p.bpm, 110.0);
        h.toggle(&mut p);
        assert_eq!(p.bpm, 100.0);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::default();
        let mut p = Project::new();
        edit(&mut h, &mut p, 100.0);
        h.undo(&mut p);
        edit(&mut h, &mut p, 90.0);
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_is_enforced() {
        let mut h = History::new(3);
        let mut p = Project::new();
        for i in 0..10 {
            edit(&mut h, &mut p, 50.0 + i as f64);
        }
        assert_eq!(h.undo_labels().count(), 3);
    }
}
