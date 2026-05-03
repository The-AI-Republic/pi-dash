//! Keymap registry — declarative `(KeyEvent, Context) → Action`
//! mapping. Resolution is a pure function in `resolver`, with the
//! defaults defined in `default_bindings`.
//!
//! Shape borrowed from claudy (`keybindings/`): contexts named, last
//! binding wins, most-specific context wins. Specificity is
//! determined by the order the caller passes contexts to `resolve` —
//! `[…tab_specific, Tabs, Global]`.

pub mod default_bindings;
pub mod resolver;

use std::collections::HashMap;

pub use resolver::{resolve, Resolution};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    /// Universal hotkeys (Ctrl+C, `?`, …).
    Global,
    /// Tab navigation (Tab/Shift+Tab/digit jumps).
    Tabs,
    /// A list pane is focused (j/k/Enter/d/etc.).
    List,
    /// A single-line text input is focused. *Excludes* `Tabs` and
    /// `Global` from the active-contexts list, so digit/letter keys
    /// can't escape into tab switches.
    TextInput,
    /// A picker submodal is open (search-as-you-type list).
    Picker,
    /// A confirm-dialog modal is open (y/n).
    ConfirmDialog,
    /// The Runners-tab settings card is focused (j/k cycles fields).
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Lifecycle
    Quit,
    QuitForce,
    StopDaemon,
    OpenHelp,
    Refresh,

    // Tab navigation
    NextTab,
    PrevTab,
    GoToTab(usize),

    // List navigation
    ListUp,
    ListDown,
    ListAccept,
    ListCancel,

    // Form
    FieldNext,
    FieldPrev,
    SubmitForm,

    // Approvals
    ApprovalAccept,
    ApprovalAcceptForSession,
    ApprovalDecline,

    // Runners-tab settings card
    SettingsToggleFocus,

    // Service controls (General tab)
    ServiceStart,
    ServiceStop,

    // Add / remove runner (Runners tab)
    OpenAddRunner,
    RemoveSelectedRunner,

    // Multi-runner picker (`<`/`>`/Alt+1..9)
    RunnerPickerPrev,
    RunnerPickerNext,
    RunnerPickerJump(usize),

    // Config save/discard
    SaveConfig,
    DiscardEdits,

    // Generic confirm
    ConfirmYes,
    ConfirmNo,
}

pub type BindingMap = HashMap<KeyMatch, Action>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyMatch {
    pub code: crossterm::event::KeyCode,
    pub mods: crossterm::event::KeyModifiers,
}

impl KeyMatch {
    pub const fn new(code: crossterm::event::KeyCode, mods: crossterm::event::KeyModifiers) -> Self {
        Self { code, mods }
    }
}

#[derive(Debug, Default)]
pub struct KeymapRegistry {
    pub blocks: Vec<(Context, BindingMap)>,
}

impl KeymapRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_blocks(blocks: Vec<(Context, Vec<(KeyMatch, Action)>)>) -> Self {
        Self {
            blocks: blocks
                .into_iter()
                .map(|(ctx, pairs)| (ctx, pairs.into_iter().collect::<HashMap<_, _>>()))
                .collect(),
        }
    }
}
