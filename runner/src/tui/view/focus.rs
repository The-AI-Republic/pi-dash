//! Focus tree + path — the recursive, layered focus model.
//!
//! Every tab declares a `Vec<FocusNode>` describing its top-level cards,
//! their grid positions, and any nested children. The App keeps one
//! `FocusPath` per tab; layer = `path.len()`. Layer 0 means focus is on
//! the tab bar; Layer N≥1 means focus is N levels deep into the tab's
//! card tree. A child of a card sits at `parent_layer + 1`.
//!
//! Two node kinds:
//!
//! - `Card` — a focusable container. Enter dives into its first child
//!   (Layer + 1) when `interactive` is true; no-op when false.
//! - `Item` — a focusable leaf. Enter triggers an action when
//!   `interactive` is true; no-op when false. Items are where most
//!   keystrokes are routed (e.g. typing into a TextArea, cycling an
//!   enum field).
//!
//! Sibling navigation respects `row` so ←/→ moves within a row and
//! ↑/↓ moves between rows. The dispatcher uses these grid coordinates
//! to compute "next sibling left/right/up/down."

/// Per-tab card identifier. Static strings so equality is cheap and
/// the ID participates in `serde` / debug output for free.
pub type CardId = &'static str;

/// One focusable element in the recursive tree.
#[derive(Debug, Clone)]
pub enum FocusNode {
    /// Container. Enter dives in (when `interactive`) — its `children`
    /// become the next layer's siblings.
    Card {
        id: CardId,
        interactive: bool,
        /// Visual-grid row this card occupies among its siblings.
        /// Same row = ←/→ neighbours. Different rows = ↑/↓ neighbours.
        row: usize,
        children: Vec<FocusNode>,
    },
    /// Leaf. Enter triggers a tab-defined action (when `interactive`);
    /// printable keys / arrows route to the tab's `handle_item_key`.
    Item {
        id: CardId,
        interactive: bool,
        row: usize,
    },
}

impl FocusNode {
    pub fn id(&self) -> CardId {
        match self {
            FocusNode::Card { id, .. } | FocusNode::Item { id, .. } => id,
        }
    }

    pub fn interactive(&self) -> bool {
        match self {
            FocusNode::Card { interactive, .. } | FocusNode::Item { interactive, .. } => {
                *interactive
            }
        }
    }

    pub fn row(&self) -> usize {
        match self {
            FocusNode::Card { row, .. } | FocusNode::Item { row, .. } => *row,
        }
    }

    pub fn is_card(&self) -> bool {
        matches!(self, FocusNode::Card { .. })
    }

    pub fn children(&self) -> &[FocusNode] {
        match self {
            FocusNode::Card { children, .. } => children,
            FocusNode::Item { .. } => &[],
        }
    }
}

/// Breadcrumb of card IDs from the tab's top-level cards down to the
/// currently-focused element. Empty = focus is on the tab bar (Layer 0).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusPath {
    segments: Vec<CardId>,
}

impl FocusPath {
    pub fn new() -> Self {
        Self::default()
    }

    /// Layer depth. 0 = tab bar; N≥1 = N segments deep.
    pub fn layer(&self) -> usize {
        self.segments.len()
    }

    pub fn is_tab_bar(&self) -> bool {
        self.segments.is_empty()
    }

    /// The leaf segment — what the user is currently focused on.
    pub fn current(&self) -> Option<CardId> {
        self.segments.last().copied()
    }

    pub fn segments(&self) -> &[CardId] {
        &self.segments
    }

    pub fn push(&mut self, id: CardId) {
        self.segments.push(id);
    }

    pub fn pop(&mut self) -> Option<CardId> {
        self.segments.pop()
    }

    /// Replace the leaf segment without changing depth — used when
    /// sibling navigation moves to a different card at the same layer.
    pub fn replace_leaf(&mut self, id: CardId) {
        if let Some(last) = self.segments.last_mut() {
            *last = id;
        } else {
            self.segments.push(id);
        }
    }

    pub fn clear(&mut self) {
        self.segments.clear();
    }
}

/// Walk a focus tree to the node at `path`. Returns `None` if any
/// segment is missing (e.g. the tree shape changed since the path was
/// recorded). The dispatcher uses this to validate `FocusPath` against
/// the freshly-rebuilt tree on every key press, snapping to the first
/// available node when the recorded path is stale.
pub fn locate<'a>(tree: &'a [FocusNode], path: &[CardId]) -> Option<&'a FocusNode> {
    let mut nodes = tree;
    let mut found: Option<&FocusNode> = None;
    for seg in path {
        let node = nodes.iter().find(|n| n.id() == *seg)?;
        found = Some(node);
        nodes = node.children();
    }
    found
}

/// Walk to the parent's sibling list of the node at `path`. Returns
/// the slice the leaf belongs to and the leaf's position in it. Used
/// by the sibling-navigation logic in the App dispatcher.
pub fn parent_siblings<'a>(
    tree: &'a [FocusNode],
    path: &[CardId],
) -> Option<(&'a [FocusNode], usize)> {
    if path.is_empty() {
        return None;
    }
    let mut nodes = tree;
    for seg in &path[..path.len() - 1] {
        let node = nodes.iter().find(|n| n.id() == *seg)?;
        nodes = node.children();
    }
    let leaf_id = *path.last()?;
    let idx = nodes.iter().position(|n| n.id() == leaf_id)?;
    Some((nodes, idx))
}

/// Style for a card border given whether it currently holds focus.
/// Used by every tab so the highlight is uniform — yellow border for
/// the focused card, default for the rest.
pub fn border_style(focused: bool) -> ratatui::style::Style {
    if focused {
        ratatui::style::Style::default().fg(ratatui::style::Color::Yellow)
    } else {
        ratatui::style::Style::default()
    }
}

/// True iff the given `card_id` is the leaf of `path` — convenience
/// for tab render code that wants to highlight one card per draw.
pub fn is_focused(path: &FocusPath, card_id: CardId) -> bool {
    path.current() == Some(card_id)
}

/// True iff `card_id` appears anywhere in the focus path. Use this
/// for nested cards that should remain highlighted when focus has
/// dived into a child (e.g. the settings card stays highlighted
/// while a settings field is being edited).
pub fn is_in_path(path: &FocusPath, card_id: CardId) -> bool {
    path.segments().contains(&card_id)
}

/// True iff the user has dived *into* this card — `card_id` is in the
/// focus path but is not the leaf, so an inner item owns input. This
/// is the visual signal for "you're editing inside this card; Esc
/// pops you back out." It's strictly tighter than `is_in_path`, which
/// stays true even when focus is on the card itself.
pub fn is_dived(path: &FocusPath, card_id: CardId) -> bool {
    let segs = path.segments();
    segs.contains(&card_id) && segs.last() != Some(&card_id)
}

/// Title-suffix marker for a card that is currently dived. Returns
/// `"* "` (asterisk + trailing space) so it can be embedded inside
/// the existing `" Title "` padding convention as `" Title * "`,
/// or `""` when the card is not dived. Keeping the marker in one
/// place avoids per-tab drift on what "dived" looks like.
pub fn dived_marker(path: &FocusPath, card_id: CardId) -> &'static str {
    if is_dived(path, card_id) { "* " } else { "" }
}

/// Format the focus path as `card › child › grandchild` for display in
/// the footer breadcrumb. Returns an empty string at Layer 0.
pub fn breadcrumb(path: &FocusPath) -> String {
    path.segments().join(" › ")
}

/// Sibling-navigation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDir {
    Left,
    Right,
    Up,
    Down,
}

/// Pick the next sibling for a directional move within a slice. ←/→
/// stays within the current row; ↑/↓ moves between rows. Returns the
/// chosen sibling's id, or `None` when no move is possible (e.g. ↑ at
/// the topmost row of the tab — caller may interpret this as "pop to
/// the tab bar").
pub fn next_sibling(siblings: &[FocusNode], current_idx: usize, dir: NavDir) -> Option<CardId> {
    let current_row = siblings.get(current_idx)?.row();
    match dir {
        NavDir::Left | NavDir::Right => {
            // Same-row siblings only, in declared order.
            let same_row: Vec<usize> = siblings
                .iter()
                .enumerate()
                .filter(|(_, n)| n.row() == current_row)
                .map(|(i, _)| i)
                .collect();
            let pos = same_row.iter().position(|i| *i == current_idx)?;
            let next_pos = match dir {
                NavDir::Left if pos == 0 => return None,
                NavDir::Left => pos - 1,
                NavDir::Right if pos + 1 >= same_row.len() => return None,
                NavDir::Right => pos + 1,
                _ => unreachable!(),
            };
            siblings.get(same_row[next_pos]).map(|n| n.id())
        }
        NavDir::Up | NavDir::Down => {
            // Move to the closest row above/below; pick the leftmost
            // node on that row (a more sophisticated pick could match
            // column position, but rows in this codebase don't have
            // misaligned columns today).
            let mut rows: Vec<usize> = siblings.iter().map(|n| n.row()).collect();
            rows.sort_unstable();
            rows.dedup();
            let pos = rows.iter().position(|r| *r == current_row)?;
            let target_row = match dir {
                NavDir::Up if pos == 0 => return None,
                NavDir::Up => rows[pos - 1],
                NavDir::Down if pos + 1 >= rows.len() => return None,
                NavDir::Down => rows[pos + 1],
                _ => unreachable!(),
            };
            siblings
                .iter()
                .find(|n| n.row() == target_row)
                .map(|n| n.id())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: CardId, row: usize) -> FocusNode {
        FocusNode::Item {
            id,
            interactive: true,
            row,
        }
    }
    fn card(id: CardId, row: usize, children: Vec<FocusNode>) -> FocusNode {
        FocusNode::Card {
            id,
            interactive: true,
            row,
            children,
        }
    }

    #[test]
    fn focus_path_layer_tracks_depth() {
        let mut p = FocusPath::new();
        assert_eq!(p.layer(), 0);
        assert!(p.is_tab_bar());
        p.push("settings");
        assert_eq!(p.layer(), 1);
        assert_eq!(p.current(), Some("settings"));
        p.push("log_level");
        assert_eq!(p.layer(), 2);
        p.pop();
        assert_eq!(p.layer(), 1);
        assert_eq!(p.current(), Some("settings"));
    }

    #[test]
    fn locate_walks_nested_tree() {
        let tree = vec![card(
            "settings",
            0,
            vec![item("log_level", 0), item("log_retention", 1)],
        )];
        let node = locate(&tree, &["settings", "log_retention"]).unwrap();
        assert_eq!(node.id(), "log_retention");
        assert!(!node.is_card());
        assert!(locate(&tree, &["settings", "missing"]).is_none());
    }

    #[test]
    fn is_dived_true_only_when_card_is_an_ancestor() {
        let mut p = FocusPath::new();
        // Layer 0 — card not in path.
        assert!(!is_dived(&p, "settings"));
        assert_eq!(dived_marker(&p, "settings"), "");

        // Layer 1, focus on the card itself — not yet dived.
        p.push("settings");
        assert!(!is_dived(&p, "settings"));
        assert_eq!(dived_marker(&p, "settings"), "");

        // Layer 2, focus on a child — dived.
        p.push("log_retention");
        assert!(is_dived(&p, "settings"));
        assert_eq!(dived_marker(&p, "settings"), "* ");
        assert!(!is_dived(&p, "log_retention")); // leaf is not dived in itself

        // Pop back to the card — marker disappears.
        p.pop();
        assert!(!is_dived(&p, "settings"));
        assert_eq!(dived_marker(&p, "settings"), "");
    }

    #[test]
    fn next_sibling_same_row_moves_horizontally() {
        let row = vec![item("a", 0), item("b", 0), item("c", 0)];
        assert_eq!(next_sibling(&row, 0, NavDir::Right), Some("b"));
        assert_eq!(next_sibling(&row, 1, NavDir::Right), Some("c"));
        assert_eq!(next_sibling(&row, 2, NavDir::Right), None);
        assert_eq!(next_sibling(&row, 1, NavDir::Left), Some("a"));
        assert_eq!(next_sibling(&row, 0, NavDir::Left), None);
    }

    #[test]
    fn next_sibling_up_down_moves_between_rows() {
        let grid = vec![
            item("top_left", 0),
            item("top_right", 0),
            item("mid", 1),
            item("bottom", 2),
        ];
        // From top_left (row 0) ↓ should land on row 1.
        assert_eq!(next_sibling(&grid, 0, NavDir::Down), Some("mid"));
        // From mid (row 1) ↑ should land on row 0's leftmost.
        assert_eq!(next_sibling(&grid, 2, NavDir::Up), Some("top_left"));
        // From bottom (row 2) ↓ has no further row.
        assert_eq!(next_sibling(&grid, 3, NavDir::Down), None);
    }

    #[test]
    fn parent_siblings_returns_leaf_index() {
        let tree = vec![card(
            "settings",
            0,
            vec![item("log_level", 0), item("log_retention", 1)],
        )];
        let (siblings, idx) = parent_siblings(&tree, &["settings", "log_retention"]).unwrap();
        assert_eq!(siblings.len(), 2);
        assert_eq!(idx, 1);
    }
}
