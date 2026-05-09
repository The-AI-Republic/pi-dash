//! Pure keymap resolution.
//!
//! `resolve(key, contexts, registry)` is a pure function returning
//! one of:
//!
//! - `Match(action)` — the key resolved to an action under one of the
//!   active contexts. The most-specific context wins; within a
//!   single context, last-binding-wins (`HashMap` overwrite).
//! - `None` — no match in any active context.
//!
//! There is no chord state today (single-keystroke bindings only).
//! `pending` is wired through so we can add chord support later
//! without changing call sites.

use crossterm::event::KeyEvent;

use super::{Action, BindingMap, Context, KeyMatch, KeymapRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Match(Action),
    None,
}

pub fn resolve(
    key: &KeyEvent,
    active_contexts: &[Context],
    registry: &KeymapRegistry,
) -> Resolution {
    let needle = KeyMatch::new(key.code, key.modifiers);
    for ctx in active_contexts {
        if let Some(map) = block_for(registry, *ctx)
            && let Some(action) = map.get(&needle)
        {
            return Resolution::Match(*action);
        }
    }
    Resolution::None
}

fn block_for(registry: &KeymapRegistry, ctx: Context) -> Option<&BindingMap> {
    registry
        .blocks
        .iter()
        .find_map(|(c, map)| if *c == ctx { Some(map) } else { None })
}

#[cfg(test)]
mod tests {
    use super::super::default_bindings;
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn digit_resolves_to_tab_switch_in_tabs_context() {
        let reg = default_bindings::defaults();
        let r = resolve(&key(KeyCode::Char('1')), &[Context::Tabs, Context::Global], &reg);
        assert_eq!(r, Resolution::Match(Action::GoToTab(0)));
    }

    #[test]
    fn digit_does_not_resolve_when_text_input_is_only_active_context() {
        let reg = default_bindings::defaults();
        let r = resolve(&key(KeyCode::Char('1')), &[Context::TextInput], &reg);
        assert_eq!(r, Resolution::None);
    }

    #[test]
    fn ctrl_c_global() {
        let reg = default_bindings::defaults();
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let r = resolve(&ev, &[Context::Global], &reg);
        assert_eq!(r, Resolution::Match(Action::Quit));
    }

    #[test]
    fn most_specific_context_wins() {
        let reg = default_bindings::defaults();
        // 'q' is bound in Global. If we put List first (which doesn't
        // bind 'q'), Global still resolves it.
        let r = resolve(&key(KeyCode::Char('q')), &[Context::List, Context::Global], &reg);
        assert_eq!(r, Resolution::Match(Action::Quit));
    }
}
