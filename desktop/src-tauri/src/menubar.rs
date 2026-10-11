// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The app menubar.
//!
//! `App::set_menu` replaces Tauri's default menu wholesale, so everything the
//! menubar must offer has to be listed here. The layout is plain data
//! ([`layout`]) so a test can check it without a display; [`build`] turns it
//! into the native menu.

use tauri::{
    AppHandle, Runtime,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder, WINDOW_SUBMENU_ID},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Separator,
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Maximize,
    CloseWindow,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomInShift,
    ZoomInNumpad,
}

const WINDOW_MENU: &str = "Window";

/// The submenus of the menubar, in order, for the given platform.
pub(crate) fn layout(macos: bool) -> Vec<(&'static str, Vec<Entry>)> {
    use Entry::*;

    // View menu: browser-style zoom shortcuts. The Tauri webview doesn't bind
    // these by default, so we register them as menu accelerators. We register
    // three variants routed to the same `zoom_in` id; alternates are
    // Linux/Windows only — on macOS, `Cmd+=` is the conventional shortcut and
    // the duplicates would visibly clutter the menubar.
    let mut view = vec![ZoomIn, ZoomOut, ZoomReset];
    if !macos {
        view.extend([ZoomInShift, ZoomInNumpad]);
        return vec![("View", view)];
    }
    view.extend([Separator, Fullscreen]);

    // macOS: the webview only gets Cmd+C/V/X/A/Z as key equivalents of the
    // Edit items, and Cmd+Q from the application menu's Quit — which is the
    // first submenu whatever its title. WebView2 and WebKitGTK handle the
    // editing keys themselves, so the other platforms keep just View.
    vec![
        (
            "Pi Dash",
            vec![
                About, Separator, Services, Separator, Hide, HideOthers, ShowAll, Separator, Quit,
            ],
        ),
        ("Edit", vec![Undo, Redo, Separator, Cut, Copy, Paste, SelectAll]),
        ("View", view),
        (WINDOW_MENU, vec![Minimize, Maximize, Separator, CloseWindow]),
    ]
}

pub(crate) fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    // muda's accelerator parser has no `Plus` token (it would silently bind
    // the `P` key); the numpad `+` is spelled `NumpadPlus`.
    let zoom = |id: &str, text: &str, accelerator: &str| {
        MenuItemBuilder::with_id(id, text)
            .accelerator(accelerator)
            .build(app)
    };

    let mut menu = MenuBuilder::new(app);
    for (title, entries) in layout(cfg!(target_os = "macos")) {
        // macOS lists the open windows in the submenu carrying this id.
        let mut submenu = if title == WINDOW_MENU {
            SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, title)
        } else {
            SubmenuBuilder::new(app, title)
        };
        for entry in entries {
            submenu = match entry {
                Entry::Separator => submenu.separator(),
                Entry::About => submenu.about(None),
                Entry::Services => submenu.services(),
                Entry::Hide => submenu.hide(),
                Entry::HideOthers => submenu.hide_others(),
                Entry::ShowAll => submenu.show_all(),
                Entry::Quit => submenu.quit(),
                Entry::Undo => submenu.undo(),
                Entry::Redo => submenu.redo(),
                Entry::Cut => submenu.cut(),
                Entry::Copy => submenu.copy(),
                Entry::Paste => submenu.paste(),
                Entry::SelectAll => submenu.select_all(),
                Entry::Fullscreen => submenu.fullscreen(),
                Entry::Minimize => submenu.minimize(),
                Entry::Maximize => submenu.maximize(),
                Entry::CloseWindow => submenu.close_window(),
                Entry::ZoomIn => submenu.item(&zoom("zoom_in", "Zoom In", "CmdOrCtrl+=")?),
                Entry::ZoomOut => submenu.item(&zoom("zoom_out", "Zoom Out", "CmdOrCtrl+-")?),
                Entry::ZoomReset => submenu.item(&zoom("zoom_reset", "Actual Size", "CmdOrCtrl+0")?),
                Entry::ZoomInShift => submenu.item(&zoom("zoom_in", "Zoom In", "CmdOrCtrl+Shift+=")?),
                Entry::ZoomInNumpad => {
                    submenu.item(&zoom("zoom_in", "Zoom In", "CmdOrCtrl+NumpadPlus")?)
                }
            };
        }
        menu = menu.item(&submenu.build()?);
    }
    menu.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(macos: bool) -> Vec<Entry> {
        layout(macos).into_iter().flat_map(|(_, e)| e).collect()
    }

    #[test]
    fn macos_menubar_keeps_the_standard_editing_and_quit_items() {
        let entries = entries(true);
        for required in [
            Entry::Undo,
            Entry::Redo,
            Entry::Cut,
            Entry::Copy,
            Entry::Paste,
            Entry::SelectAll,
            Entry::Quit,
        ] {
            assert!(entries.contains(&required), "macOS menubar is missing {required:?}");
        }
    }

    #[test]
    fn macos_application_menu_comes_first_and_holds_quit() {
        let layout = layout(true);
        assert!(layout[0].1.contains(&Entry::Quit));
    }

    #[test]
    fn zoom_items_are_on_every_platform() {
        for macos in [true, false] {
            let entries = entries(macos);
            for required in [Entry::ZoomIn, Entry::ZoomOut, Entry::ZoomReset] {
                assert!(entries.contains(&required), "macos={macos}: missing {required:?}");
            }
        }
        assert!(!entries(true).contains(&Entry::ZoomInShift));
        assert!(entries(false).contains(&Entry::ZoomInNumpad));
    }

    #[test]
    fn non_macos_menubar_does_not_claim_the_editing_shortcuts() {
        let entries = entries(false);
        for unwanted in [Entry::Copy, Entry::Paste, Entry::Cut, Entry::Quit] {
            assert!(!entries.contains(&unwanted), "unexpected {unwanted:?}");
        }
    }
}
