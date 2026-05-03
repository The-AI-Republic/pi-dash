//! `SelectableList<Id>` — a list cursor that survives data refreshes
//! by tracking the *identity* of the selected item, not its index.
//!
//! This is the runner-side port of claudy's `validatedFocusedValue`
//! (`use-select-navigation.ts:592-602`): when the underlying items
//! are reloaded, we re-derive the cursor's index from its id. If the
//! id is gone, we fall back to first-item.
//!
//! Combined with `ScrollState`, this fixes the runner's "selection
//! resets on every tick" bug (Bug 2): `state.selected = 0` was
//! reassigned in many places after a refresh, so the user could only
//! ever interact with item 0. With `SelectableList<Id>`, the dispatcher
//! calls `reconcile(items)` on the new list and selection follows the
//! original element across re-orderings, additions, deletions.

use super::scroll_state::ScrollState;

#[derive(Debug, Clone)]
pub struct SelectableList<Id: Clone + Eq> {
    state: ScrollState,
    selected_id: Option<Id>,
}

impl<Id: Clone + Eq> Default for SelectableList<Id> {
    fn default() -> Self {
        Self {
            state: ScrollState::default(),
            selected_id: None,
        }
    }
}

impl<Id: Clone + Eq> SelectableList<Id> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-anchor the cursor against a freshly-loaded item list. If
    /// the previously selected id is still present, the index snaps
    /// to its new position; otherwise we fall back to the clamped
    /// `selected` index (or 0 if the list became empty).
    pub fn reconcile(&mut self, items: &[Id]) {
        if items.is_empty() {
            self.state.selected = None;
            self.state.scroll_top = 0;
            self.selected_id = None;
            return;
        }
        if let Some(id) = self.selected_id.as_ref()
            && let Some(pos) = items.iter().position(|x| x == id)
        {
            self.state.selected = Some(pos);
            return;
        }
        // Id is gone — fall back to clamping the previous index.
        self.state.clamp(items.len());
        self.selected_id = self.state.selected.and_then(|i| items.get(i).cloned());
    }

    pub fn move_up(&mut self, items: &[Id]) {
        self.state.move_up_wrap(items.len());
        self.selected_id = self.state.selected.and_then(|i| items.get(i).cloned());
    }

    pub fn move_down(&mut self, items: &[Id]) {
        self.state.move_down_wrap(items.len());
        self.selected_id = self.state.selected.and_then(|i| items.get(i).cloned());
    }

    pub fn jump_to(&mut self, idx: usize, items: &[Id]) {
        if items.is_empty() {
            self.state.selected = None;
            self.selected_id = None;
            return;
        }
        let i = idx.min(items.len() - 1);
        self.state.selected = Some(i);
        self.selected_id = items.get(i).cloned();
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected
    }

    pub fn selected_id(&self) -> Option<&Id> {
        self.selected_id.as_ref()
    }

    pub fn ensure_visible(&mut self, len: usize, rows: usize) {
        self.state.ensure_visible(len, rows);
    }

    pub fn scroll_top(&self) -> usize {
        self.state.scroll_top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_preserves_id_after_reorder() {
        let mut list = SelectableList::<String>::new();
        list.reconcile(&["a".into(), "b".into(), "c".into()]);
        list.move_down(&["a".into(), "b".into(), "c".into()]);
        assert_eq!(list.selected_id(), Some(&"b".to_string()));
        // Reorder: b moves to index 0
        list.reconcile(&["b".into(), "a".into(), "c".into()]);
        assert_eq!(list.selected_index(), Some(0));
        assert_eq!(list.selected_id(), Some(&"b".to_string()));
    }

    #[test]
    fn reconcile_after_id_removed_falls_back_to_clamped_index() {
        let mut list = SelectableList::<String>::new();
        list.reconcile(&["a".into(), "b".into(), "c".into()]);
        list.move_down(&["a".into(), "b".into(), "c".into()]);
        assert_eq!(list.selected_id(), Some(&"b".to_string()));
        // b removed
        list.reconcile(&["a".into(), "c".into()]);
        // Index 1 was clamped — falls back to the new index 1 ("c").
        assert_eq!(list.selected_index(), Some(1));
        assert_eq!(list.selected_id(), Some(&"c".to_string()));
    }

    #[test]
    fn reconcile_empty_clears_selection() {
        let mut list = SelectableList::<String>::new();
        list.reconcile(&["a".into()]);
        list.reconcile(&[]);
        assert_eq!(list.selected_index(), None);
        assert_eq!(list.selected_id(), None);
    }

    #[test]
    fn move_wraps_around_ends() {
        let mut list = SelectableList::<u32>::new();
        list.reconcile(&[1, 2, 3]);
        list.move_up(&[1, 2, 3]); // wraps from 0 → 2
        assert_eq!(list.selected_id(), Some(&3));
        list.move_down(&[1, 2, 3]); // wraps 2 → 0
        assert_eq!(list.selected_id(), Some(&1));
    }
}
