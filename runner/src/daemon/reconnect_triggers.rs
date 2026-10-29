//! Reconnect triggers (PDASHOSS01-306).
//!
//! While the cloud is unreachable each runner backs off up to 6 hours
//! between attempts, so a runner could stay offline for hours after the
//! server recovers. These background watchers cut the wait short when
//! something suggests connectivity just changed:
//!
//! - **wake from sleep** — detected as wall-clock time jumping ahead of
//!   monotonic time. Both `Instant` and tokio timers stop while the machine
//!   sleeps (on macOS and Linux), but the wall clock keeps going.
//! - **network change** — the set of non-loopback, non-link-local
//!   interface addresses changed (interface up/down, new IP). Unix only.
//!   Link-local addresses are ignored: every Docker container start/stop
//!   adds or drops a veth with an `fe80::` address, and counting those
//!   would reset the outage backoff on every container lifecycle.
//!
//! `pidash reconnect` (IPC) and `pidash restart` (a fresh process) are the
//! other two triggers; they need no watcher.

use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime};

/// How often the watchers sample.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(10);
/// Wall-clock time must run ahead of monotonic time by more than this
/// between two samples to count as a wake. Large enough to ignore NTP
/// slews and scheduler hiccups.
const WAKE_THRESHOLD: Duration = Duration::from_secs(30);

/// Spawn the wake and network-change watchers. `kick` runs with a short
/// label for the log whenever one fires.
pub fn spawn(kick: impl Fn(&'static str) + Send + Sync + 'static) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut wall = SystemTime::now();
        let mut mono = Instant::now();
        let mut addrs = interface_fingerprint();
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let (now_wall, now_mono) = (SystemTime::now(), Instant::now());
            let woke = is_wake(
                now_wall.duration_since(wall).unwrap_or_default(),
                now_mono.saturating_duration_since(mono),
            );
            wall = now_wall;
            mono = now_mono;
            let now_addrs = interface_fingerprint();
            let network_changed = addresses_changed(addrs.as_ref(), now_addrs.as_ref());
            addrs = now_addrs;
            if woke {
                tracing::info!("system woke from sleep; retrying cloud connections now");
                kick("wake");
            } else if network_changed {
                tracing::info!("network changed; retrying cloud connections now");
                kick("network change");
            }
        }
    })
}

/// True when the wall clock advanced by more than [`WAKE_THRESHOLD`] beyond
/// the monotonic clock over the same interval.
pub fn is_wake(wall_elapsed: Duration, mono_elapsed: Duration) -> bool {
    wall_elapsed.saturating_sub(mono_elapsed) > WAKE_THRESHOLD
}

/// True when both samples exist and differ. A failed sample (`None`) never
/// counts as a change.
pub fn addresses_changed(
    before: Option<&BTreeSet<String>>,
    after: Option<&BTreeSet<String>>,
) -> bool {
    matches!((before, after), (Some(b), Some(a)) if a != b)
}

/// Loopback and link-local addresses say nothing about reaching the cloud
/// and churn with local virtual interfaces, so they are left out of the
/// fingerprint.
pub fn counts_for_network_change(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local(),
        std::net::IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unicast_link_local(),
    }
}

/// Sorted `interface=address` pairs for every routable IP address.
#[cfg(unix)]
fn interface_fingerprint() -> Option<BTreeSet<String>> {
    let addrs = nix::ifaddrs::getifaddrs().ok()?;
    let mut out = BTreeSet::new();
    for ifa in addrs {
        let Some(address) = ifa.address else {
            continue;
        };
        let ip = if let Some(v4) = address.as_sockaddr_in() {
            std::net::IpAddr::V4(v4.ip())
        } else if let Some(v6) = address.as_sockaddr_in6() {
            std::net::IpAddr::V6(v6.ip())
        } else {
            continue;
        };
        if !counts_for_network_change(ip) {
            continue;
        }
        out.insert(format!("{}={ip}", ifa.interface_name));
    }
    Some(out)
}

/// Network-change detection is not implemented on this platform; wake
/// detection and `pidash reconnect` still apply.
#[cfg(not(unix))]
fn interface_fingerprint() -> Option<BTreeSet<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_jump_is_a_wake() {
        let s = Duration::from_secs;
        assert!(is_wake(s(3600 + 10), s(10)));
        assert!(!is_wake(s(10), s(10)));
        assert!(!is_wake(s(12), s(10)), "small drift is not a wake");
        assert!(
            !is_wake(s(5), s(10)),
            "wall clock stepping back is not a wake"
        );
    }

    #[test]
    fn address_set_change_is_a_network_change() {
        let a: BTreeSet<String> = ["en0=192.168.1.5".to_string()].into();
        let b: BTreeSet<String> = ["en0=10.0.0.7".to_string()].into();
        assert!(addresses_changed(Some(&a), Some(&b)));
        assert!(!addresses_changed(Some(&a), Some(&a.clone())));
        assert!(!addresses_changed(None, Some(&a)));
        assert!(!addresses_changed(Some(&a), None));
    }

    #[test]
    fn link_local_and_loopback_addresses_are_not_network_changes() {
        let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
        assert!(!counts_for_network_change(ip("127.0.0.1")));
        assert!(!counts_for_network_change(ip("::1")));
        assert!(!counts_for_network_change(ip("169.254.10.2")));
        assert!(!counts_for_network_change(ip("fe80::1c2:3ff:fe44:5566")));
        assert!(counts_for_network_change(ip("192.168.1.5")));
        assert!(counts_for_network_change(ip("2001:db8::7")));
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_reads_interfaces_without_loopback() {
        let fp = interface_fingerprint().expect("getifaddrs works on unix");
        assert!(
            fp.iter().all(|e| !e.ends_with("=127.0.0.1")
                && !e.ends_with("=::1")
                && !e.contains("=fe80:"))
        );
    }
}
