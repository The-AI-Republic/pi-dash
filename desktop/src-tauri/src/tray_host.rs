// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Whether the tray icon is actually on screen.
//!
//! Closing the main window hides it to the tray, which leaves the tray's
//! "Show" and "Quit" as the way back. On Linux the tray is an AppIndicator:
//! it is drawn by a StatusNotifier host (KDE, or GNOME with an AppIndicator
//! extension) or, when no StatusNotifier watcher exists, by the library's
//! fallback XEmbed status icon in an X11 system tray. Stock GNOME has
//! neither, and building the tray still succeeds there, so the icon is
//! silently missing. Windows and macOS always show it.

/// Whether a tray icon would be visible to the user right now.
///
/// Checked at close time rather than at launch, because a tray host can
/// appear or go away while the app runs (an extension enabled, a panel
/// restarted).
#[cfg(target_os = "linux")]
pub(crate) fn tray_icon_visible() -> bool {
    sni_host_registered() || xembed_tray_present()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn tray_icon_visible() -> bool {
    true
}

#[cfg(target_os = "linux")]
const SNI_WATCHER: &str = "org.kde.StatusNotifierWatcher";

#[cfg(target_os = "linux")]
fn sni_host_registered() -> bool {
    zbus::blocking::Connection::session().is_ok_and(|conn| sni_host_registered_on(&conn))
}

/// Whether the session bus has a StatusNotifier watcher with a host
/// registered — the host is what draws the icon; a watcher alone does not.
#[cfg(target_os = "linux")]
fn sni_host_registered_on(conn: &zbus::blocking::Connection) -> bool {
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(conn)
        .destination(SNI_WATCHER)
        .and_then(|b| b.path("/StatusNotifierWatcher"))
        .and_then(|b| b.interface(SNI_WATCHER))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    proxy
        .and_then(|p| p.get_property::<bool>("IsStatusNotifierHostRegistered"))
        .unwrap_or(false)
}

/// Whether an X11 system tray manager owns the `_NET_SYSTEM_TRAY_S<n>`
/// selection, which is where the fallback status icon goes.
#[cfg(target_os = "linux")]
fn xembed_tray_present() -> bool {
    use x11_dl::xlib::{False, Xlib};

    let Ok(xlib) = Xlib::open() else {
        return false;
    };
    // SAFETY: plain Xlib calls on a display connection this function opens
    // and closes itself; nothing outlives it.
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return false;
        }
        let screen = (xlib.XDefaultScreen)(display);
        let name = std::ffi::CString::new(format!("_NET_SYSTEM_TRAY_S{screen}"))
            .expect("atom name has no NUL");
        let atom = (xlib.XInternAtom)(display, name.as_ptr(), False);
        let owned = atom != 0 && (xlib.XGetSelectionOwner)(display, atom) != 0;
        (xlib.XCloseDisplay)(display);
        owned
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    /// A private session bus, killed on drop.
    struct Bus(Child, String);

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// `None` when `dbus-daemon` is not installed.
    fn private_bus() -> Option<Bus> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut address = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        Some(Bus(child, address.trim().to_owned()))
    }

    fn connect(bus: &Bus) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(bus.1.as_str())
            .unwrap()
            .build()
            .unwrap()
    }

    struct Watcher {
        host: bool,
    }

    #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
    impl Watcher {
        #[zbus(property)]
        fn is_status_notifier_host_registered(&self) -> bool {
            self.host
        }
    }

    /// Serves a StatusNotifier watcher on the bus, as a desktop's tray would.
    fn serve_watcher(bus: &Bus, host: bool) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(bus.1.as_str())
            .unwrap()
            .name(SNI_WATCHER)
            .unwrap()
            .serve_at("/StatusNotifierWatcher", Watcher { host })
            .unwrap()
            .build()
            .unwrap()
    }

    macro_rules! bus_or_skip {
        () => {
            match private_bus() {
                Some(bus) => bus,
                None => {
                    eprintln!("skipped: dbus-daemon not installed");
                    return;
                }
            }
        };
    }

    #[test]
    fn no_watcher_means_no_tray() {
        // Stock GNOME: nothing on the bus draws an AppIndicator.
        let bus = bus_or_skip!();
        assert!(!sni_host_registered_on(&connect(&bus)));
    }

    #[test]
    fn watcher_with_host_means_tray() {
        let bus = bus_or_skip!();
        let _watcher = serve_watcher(&bus, true);
        assert!(sni_host_registered_on(&connect(&bus)));
    }

    #[test]
    fn watcher_without_host_means_no_tray() {
        let bus = bus_or_skip!();
        let _watcher = serve_watcher(&bus, false);
        assert!(!sni_host_registered_on(&connect(&bus)));
    }
}
