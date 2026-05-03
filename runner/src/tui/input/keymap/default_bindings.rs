//! Default keymap. Every hotkey in the runner TUI lives here.

use crossterm::event::{KeyCode, KeyModifiers};

use super::{Action, Context, KeyMatch, KeymapRegistry};

fn k(code: KeyCode) -> KeyMatch {
    KeyMatch::new(code, KeyModifiers::NONE)
}

fn km(code: KeyCode, mods: KeyModifiers) -> KeyMatch {
    KeyMatch::new(code, mods)
}

pub fn defaults() -> KeymapRegistry {
    KeymapRegistry::from_blocks(vec![
        (
            Context::Global,
            vec![
                (km(KeyCode::Char('c'), KeyModifiers::CONTROL), Action::Quit),
                (k(KeyCode::Char('q')), Action::Quit),
                (k(KeyCode::Char('Q')), Action::StopDaemon),
                (k(KeyCode::Char('?')), Action::OpenHelp),
                (k(KeyCode::Char('r')), Action::Refresh),
            ],
        ),
        (
            Context::Tabs,
            vec![
                (k(KeyCode::Tab), Action::NextTab),
                (k(KeyCode::BackTab), Action::PrevTab),
                (k(KeyCode::Char('h')), Action::PrevTab),
                (k(KeyCode::Char('l')), Action::NextTab),
                (k(KeyCode::Left), Action::PrevTab),
                (k(KeyCode::Right), Action::NextTab),
                (k(KeyCode::Char('1')), Action::GoToTab(0)),
                (k(KeyCode::Char('2')), Action::GoToTab(1)),
                (k(KeyCode::Char('3')), Action::GoToTab(2)),
                (k(KeyCode::Char('4')), Action::GoToTab(3)),
            ],
        ),
        (
            Context::List,
            vec![
                (k(KeyCode::Char('j')), Action::ListDown),
                (k(KeyCode::Char('k')), Action::ListUp),
                (k(KeyCode::Down), Action::ListDown),
                (k(KeyCode::Up), Action::ListUp),
                (k(KeyCode::Enter), Action::ListAccept),
                (k(KeyCode::Esc), Action::ListCancel),
                (k(KeyCode::Char('<')), Action::RunnerPickerPrev),
                (k(KeyCode::Char(',')), Action::RunnerPickerPrev),
                (k(KeyCode::Char('>')), Action::RunnerPickerNext),
                (k(KeyCode::Char('.')), Action::RunnerPickerNext),
                (km(KeyCode::Char('1'), KeyModifiers::ALT), Action::RunnerPickerJump(0)),
                (km(KeyCode::Char('2'), KeyModifiers::ALT), Action::RunnerPickerJump(1)),
                (km(KeyCode::Char('3'), KeyModifiers::ALT), Action::RunnerPickerJump(2)),
                (km(KeyCode::Char('4'), KeyModifiers::ALT), Action::RunnerPickerJump(3)),
                (km(KeyCode::Char('5'), KeyModifiers::ALT), Action::RunnerPickerJump(4)),
                (km(KeyCode::Char('6'), KeyModifiers::ALT), Action::RunnerPickerJump(5)),
                (km(KeyCode::Char('7'), KeyModifiers::ALT), Action::RunnerPickerJump(6)),
                (km(KeyCode::Char('8'), KeyModifiers::ALT), Action::RunnerPickerJump(7)),
                (km(KeyCode::Char('9'), KeyModifiers::ALT), Action::RunnerPickerJump(8)),
            ],
        ),
        (
            Context::Settings,
            vec![
                (k(KeyCode::Char('j')), Action::ListDown),
                (k(KeyCode::Char('k')), Action::ListUp),
                (k(KeyCode::Down), Action::ListDown),
                (k(KeyCode::Up), Action::ListUp),
                (k(KeyCode::Enter), Action::ListAccept),
                (k(KeyCode::Esc), Action::DiscardEdits),
                (k(KeyCode::Char('w')), Action::SaveConfig),
                (k(KeyCode::Tab), Action::SettingsToggleFocus),
                (k(KeyCode::BackTab), Action::SettingsToggleFocus),
            ],
        ),
        (
            Context::ConfirmDialog,
            vec![
                (k(KeyCode::Char('y')), Action::ConfirmYes),
                (k(KeyCode::Char('Y')), Action::ConfirmYes),
                (k(KeyCode::Char('n')), Action::ConfirmNo),
                (k(KeyCode::Char('N')), Action::ConfirmNo),
                (k(KeyCode::Esc), Action::ConfirmNo),
                (k(KeyCode::Enter), Action::ConfirmYes),
            ],
        ),
        // TextInput: intentionally empty. When the active contexts list
        // is `[TextInput]` only, every Char/Enter/Esc falls through to
        // the textarea / form handler — no keymap action can intercept.
        (Context::TextInput, vec![]),
        // Picker: intentionally empty for the same reason. The picker
        // owns its own typing surface.
        (Context::Picker, vec![]),
    ])
}
