//! SSRF guard for BYOK `base_url` values (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/ssrf.py:1-43`: enabled in cloud
//! (`ASSISTANT_BLOCK_PRIVATE_URLS=True`), off by default in OSS. Fixture id
//! F-A6-05 (`rust-api/fixtures/assistant/ssrf.json`).
//!
//! The setting crosses as a plain `bool` (Django `settings` never reach
//! library code); DNS crosses as an injected [`HostResolver`] so unit tests
//! stay hermetic (the `GitLabTransport` precedent) — the verdict battery
//! below replays live CPython 3.12 verdicts without touching the network.
//!
//! Verdict semantics replicate CPython 3.12's `ipaddress` tables, read from
//! the 3.12 source (`Lib/ipaddress.py`) rather than re-derived: an address
//! is blocked when any of `is_private | is_loopback | is_link_local |
//! is_reserved | is_multicast` holds, over every `getaddrinfo` result, with
//! unresolvable hosts, empty hostnames, and unparseable addresses all
//! blocked (`ssrf.py:25-43`). IPv4-mapped IPv6 addresses delegate all five
//! predicates to the embedded IPv4 address (`ipaddress.py:2054-2114`).
//!
//! Ported behaviors worth knowing (each asserted below):
//!
//! * `100.64.0.0/10` (CGNAT shared space) is *not* private and passes the
//!   guard; `192.0.0.9`/`.10` are carved out of `192.0.0.0/24` the same way.
//! * A URL without a parseable hostname (`example.com` with no scheme,
//!   `""`, unbracketed `::1`) is blocked, not an error.
//! * A hostname that fails to parse as an IP after resolution (scope ids,
//!   `getaddrinfo` legacy forms like `0x7f.0.0.1` that DNS cannot confirm)
//!   is blocked via the `ValueError` branch.

/// Whether private-URL blocking is on (`ssrf.py:21-22`,
/// `ASSISTANT_BLOCK_PRIVATE_URLS`, default `False`).
pub fn blocking_enabled(block_private_urls: bool) -> bool {
    block_private_urls
}

/// Extract the hostname the guard resolves (`urlparse(url).hostname`,
/// `ssrf.py:29`): the `://` authority sans userinfo, port, and `[]`
/// brackets, lowercased. Returns `None` exactly where Python yields no
/// hostname (no scheme, empty authority, unbracketed multi-colon hosts) —
/// plus unclosed brackets, where Python raises `ValueError` and this port
/// fails closed (see [`is_blocked`]).
pub fn extract_hostname(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme
        .split_terminator(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    if hostport.is_empty() {
        return None;
    }
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        // Bracketed IPv6 literal; the port (if any) follows `]`.
        let end = rest.find(']')?;
        rest[..end].to_owned()
    } else {
        if hostport.chars().filter(|c| *c == ':').count() > 1 {
            // Unbracketed multi-colon host: `urlparse().hostname` is `None`.
            return None;
        }
        hostport.split(':').next().unwrap_or("").to_owned()
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

/// DNS for [`is_blocked`]: `socket.getaddrinfo(host, None)` (`ssrf.py:33`).
/// The default [`SystemResolver`] calls libc resolution with port 0 (no
/// service); numeric hosts never emit DNS traffic. Tests inject fakes.
pub trait HostResolver {
    /// Resolve `host` to address strings, or `Err` when unresolvable
    /// (`socket.gaierror -> blocked`, `ssrf.py:34-35`).
    fn resolve(&self, host: &str) -> Result<Vec<String>, ResolveError>;
}

/// Unresolvable hostname (`socket.gaierror`, `ssrf.py:34-35`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveError;

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "host did not resolve")
    }
}

impl std::error::Error for ResolveError {}

/// libc resolution (`socket.getaddrinfo`, `ssrf.py:33`). A single trailing
/// dot is stripped first (absolute DNS name; `getaddrinfo` accepts
/// `8.8.8.8.` while `IpAddr::from_str` does not).
pub struct SystemResolver;

impl HostResolver for SystemResolver {
    fn resolve(&self, host: &str) -> Result<Vec<String>, ResolveError> {
        use std::net::ToSocketAddrs as _;
        let normalized = host.strip_suffix('.').unwrap_or(host);
        if normalized.is_empty() {
            return Err(ResolveError);
        }
        let mut addresses: Vec<String> = (normalized, 0)
            .to_socket_addrs()
            .map_err(|_| ResolveError)?
            .map(|socket| socket.ip().to_string())
            .collect();
        addresses.sort();
        addresses.dedup();
        if addresses.is_empty() {
            return Err(ResolveError);
        }
        Ok(addresses)
    }
}

const fn v4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) << 24 | (b as u32) << 16 | (c as u32) << 8 | d as u32
}

fn in_v4_net(ip: u32, base: u32, prefix: u8) -> bool {
    let shift = 32 - prefix;
    (ip >> shift) == (base >> shift)
}

const fn v6(segments: [u16; 8]) -> u128 {
    (segments[0] as u128) << 112
        | (segments[1] as u128) << 96
        | (segments[2] as u128) << 80
        | (segments[3] as u128) << 64
        | (segments[4] as u128) << 48
        | (segments[5] as u128) << 32
        | (segments[6] as u128) << 16
        | segments[7] as u128
}

fn in_v6_net(ip: u128, base: u128, prefix: u8) -> bool {
    let shift = 128 - prefix;
    (ip >> shift) == (base >> shift)
}

// IPv4 `is_private` (`ipaddress.py:1581-1599`): not-globally-reachable
// blocks, minus the two carved-out globals.
const V4_PRIVATE: &[(u32, u8)] = &[
    (v4(0, 0, 0, 0), 8),
    (v4(10, 0, 0, 0), 8),
    (v4(127, 0, 0, 0), 8),
    (v4(169, 254, 0, 0), 16),
    (v4(172, 16, 0, 0), 12),
    (v4(192, 0, 0, 0), 24),
    (v4(192, 0, 0, 170), 31),
    (v4(192, 0, 2, 0), 24),
    (v4(192, 168, 0, 0), 16),
    (v4(198, 18, 0, 0), 15),
    (v4(198, 51, 100, 0), 24),
    (v4(203, 0, 113, 0), 24),
    (v4(240, 0, 0, 0), 4),
    (v4(255, 255, 255, 255), 32),
];

fn v4_is_private(ip: u32) -> bool {
    if ip == v4(192, 0, 0, 9) || ip == v4(192, 0, 0, 10) {
        return false;
    }
    V4_PRIVATE
        .iter()
        .any(|(base, prefix)| in_v4_net(ip, *base, *prefix))
}

// IPv6 `is_private` (`ipaddress.py:2390-2413`).
const V6_PRIVATE: &[(u128, u8)] = &[
    (v6([0, 0, 0, 0, 0, 0, 0, 1]), 128),
    (v6([0, 0, 0, 0, 0, 0, 0, 0]), 128),
    (v6([0, 0, 0, 0, 0, 0xffff, 0, 0]), 96),
    (v6([0x64, 0xff9b, 1, 0, 0, 0, 0, 0]), 48),
    (v6([0x100, 0, 0, 0, 0, 0, 0, 0]), 64),
    (v6([0x2001, 0, 0, 0, 0, 0, 0, 0]), 23),
    (v6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0]), 32),
    (v6([0x2002, 0, 0, 0, 0, 0, 0, 0]), 16),
    (v6([0x3fff, 0, 0, 0, 0, 0, 0, 0]), 20),
    (v6([0xfc00, 0, 0, 0, 0, 0, 0, 0]), 7),
    (v6([0xfe80, 0, 0, 0, 0, 0, 0, 0]), 10),
];

fn v6_is_private(ip: u128) -> bool {
    const EXCEPTIONS: &[(u128, u8)] = &[
        (v6([0x2001, 1, 0, 0, 0, 0, 0, 1]), 128),
        (v6([0x2001, 1, 0, 0, 0, 0, 0, 2]), 128),
        (v6([0x2001, 3, 0, 0, 0, 0, 0, 0]), 32),
        (v6([0x2001, 4, 0x112, 0, 0, 0, 0, 0]), 48),
        (v6([0x2001, 0x20, 0, 0, 0, 0, 0, 0]), 28),
        (v6([0x2001, 0x30, 0, 0, 0, 0, 0, 0]), 28),
    ];
    V6_PRIVATE
        .iter()
        .any(|(base, prefix)| in_v6_net(ip, *base, *prefix))
        && !EXCEPTIONS
            .iter()
            .any(|(base, prefix)| in_v6_net(ip, *base, *prefix))
}

// IPv6 `is_reserved` (`ipaddress.py:2415-2424`).
const V6_RESERVED: &[(u128, u8)] = &[
    (v6([0, 0, 0, 0, 0, 0, 0, 0]), 8),
    (v6([0x100, 0, 0, 0, 0, 0, 0, 0]), 8),
    (v6([0x200, 0, 0, 0, 0, 0, 0, 0]), 7),
    (v6([0x400, 0, 0, 0, 0, 0, 0, 0]), 6),
    (v6([0x800, 0, 0, 0, 0, 0, 0, 0]), 5),
    (v6([0x1000, 0, 0, 0, 0, 0, 0, 0]), 4),
    (v6([0x4000, 0, 0, 0, 0, 0, 0, 0]), 3),
    (v6([0x6000, 0, 0, 0, 0, 0, 0, 0]), 3),
    (v6([0x8000, 0, 0, 0, 0, 0, 0, 0]), 3),
    (v6([0xa000, 0, 0, 0, 0, 0, 0, 0]), 3),
    (v6([0xc000, 0, 0, 0, 0, 0, 0, 0]), 3),
    (v6([0xe000, 0, 0, 0, 0, 0, 0, 0]), 4),
    (v6([0xf000, 0, 0, 0, 0, 0, 0, 0]), 5),
    (v6([0xf800, 0, 0, 0, 0, 0, 0, 0]), 6),
    (v6([0xfe00, 0, 0, 0, 0, 0, 0, 0]), 9),
];

/// Whether one resolved address string is blocked (`ssrf.py:36-42`).
/// Unparseable strings are blocked (the `except ValueError` branch).
pub fn address_is_blocked(address: &str) -> bool {
    use std::net::IpAddr;
    use std::str::FromStr as _;
    match IpAddr::from_str(address) {
        Ok(IpAddr::V4(addr)) => {
            let ip: u32 = addr.into();
            v4_is_private(ip)
                || in_v4_net(ip, v4(127, 0, 0, 0), 8)
                || in_v4_net(ip, v4(169, 254, 0, 0), 16)
                || in_v4_net(ip, v4(224, 0, 0, 0), 4)
                || in_v4_net(ip, v4(240, 0, 0, 0), 4)
        }
        Ok(IpAddr::V6(addr)) => {
            let ip: u128 = addr.into();
            // IPv4-mapped (`::ffff:0:0/96`): every predicate delegates to
            // the embedded IPv4 address (`ipaddress.py:2054-2174`).
            if ip >> 32 == 0xffff {
                return address_is_blocked(&format!(
                    "{}.{}.{}.{}",
                    (ip & 0xff000000) >> 24,
                    (ip & 0xff0000) >> 16,
                    (ip & 0xff00) >> 8,
                    ip & 0xff
                ));
            }
            v6_is_private(ip)
                || ip == 1
                || in_v6_net(ip, v6([0xfe80, 0, 0, 0, 0, 0, 0, 0]), 10)
                || in_v6_net(ip, v6([0xff00, 0, 0, 0, 0, 0, 0, 0]), 8)
                || V6_RESERVED
                    .iter()
                    .any(|(base, prefix)| in_v6_net(ip, *base, *prefix))
        }
        Err(_) => true,
    }
}

/// Whether `url` is blocked (`ssrf.py:25-43`): `False` when blocking is
/// disabled; otherwise `True` for empty/unparseable hostnames, unresolvable
/// hosts, and hosts with any blocked resolved address. Unparseable URLs
/// (unclosed brackets, where Python raises `ValueError` out of `urlparse`)
/// fail closed to `True`: every such URL also fails the serializer
/// `http(s)` validators upstream, so the raise is unreachable in practice.
pub fn is_blocked(url: &str, block_private_urls: bool, resolver: &dyn HostResolver) -> bool {
    if !blocking_enabled(block_private_urls) {
        return false;
    }
    let Some(host) = extract_hostname(url) else {
        return true;
    };
    match resolver.resolve(&host) {
        Ok(addresses) if !addresses.is_empty() => {
            addresses.iter().any(|address| address_is_blocked(address))
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/ssrf.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// Fake DNS: canned address lists; missing hosts are unresolvable.
    struct FakeResolver(HashMap<&'static str, Vec<&'static str>>);

    impl HostResolver for FakeResolver {
        fn resolve(&self, host: &str) -> Result<Vec<String>, ResolveError> {
            self.0
                .get(host)
                .map(|addresses| addresses.iter().map(ToString::to_string).collect())
                .ok_or(ResolveError)
        }
    }

    fn fake(pairs: &[(&'static str, Vec<&'static str>)]) -> FakeResolver {
        FakeResolver(pairs.iter().cloned().collect())
    }

    #[test]
    fn blocking_disabled_passes_everything() {
        assert!(!blocking_enabled(false));
        assert!(blocking_enabled(true));
        let resolver = fake(&[]);
        for url in [
            "http://127.0.0.1/",
            "http://10.0.0.1/",
            "",
            "example.com",
            "http://unresolvable/",
        ] {
            assert!(!is_blocked(url, false, &resolver), "{url} passes when off");
        }
        assert!(!fixture()["blocking_enabled"]["vectors"]["enabled"]
            .as_bool()
            .expect("flag"));
    }

    #[test]
    fn fixture_verdict_matrix() {
        let vectors = &fixture()["is_blocked"]["vectors_enabled"];
        let resolver = fake(&[
            ("8.8.8.8", vec!["8.8.8.8"]),
            ("127.0.0.1", vec!["127.0.0.1"]),
            ("10.0.0.1", vec!["10.0.0.1"]),
            ("192.168.1.1", vec!["192.168.1.1"]),
            ("169.254.169.254", vec!["169.254.169.254"]),
            ("224.0.0.1", vec!["224.0.0.1"]),
            ("240.0.0.1", vec!["240.0.0.1"]),
            ("::1", vec!["::1"]),
        ]);
        let cases = [
            ("http://8.8.8.8/", "public_ip"),
            ("http://127.0.0.1/", "loopback"),
            ("http://10.0.0.1/", "private10"),
            ("http://192.168.1.1/", "private192"),
            ("http://169.254.169.254/", "linklocal"),
            ("http://224.0.0.1/", "multicast"),
            ("http://240.0.0.1/", "reserved"),
            ("http://[::1]/", "ipv6_loop"),
            ("http://unresolvable.invalid/", "unresolvable"),
            ("", "empty"),
            ("example.com", "no_scheme_host"),
        ];
        for (url, key) in cases {
            assert_eq!(
                is_blocked(url, true, &resolver),
                vectors[key].as_bool().expect("vector"),
                "{url} ({key})"
            );
        }
    }

    #[test]
    fn hostname_extraction_mirrors_urlparse() {
        assert_eq!(
            extract_hostname("http://user:pass@10.0.0.1:8080/x").as_deref(),
            Some("10.0.0.1")
        );
        assert_eq!(
            extract_hostname("http://[::1]:8080/x").as_deref(),
            Some("::1")
        );
        assert_eq!(
            extract_hostname("http://[FE80::1]/").as_deref(),
            Some("fe80::1")
        );
        assert_eq!(
            extract_hostname("HTTP://Example.COM/").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            extract_hostname("http://8.8.8.8./").as_deref(),
            Some("8.8.8.8.")
        );
        assert_eq!(extract_hostname("http://h:bad/").as_deref(), Some("h"));
        assert_eq!(extract_hostname("example.com"), None);
        assert_eq!(extract_hostname(""), None);
        assert_eq!(extract_hostname("http://"), None);
        assert_eq!(extract_hostname("http://::1/"), None);
        assert_eq!(extract_hostname("http://[::1"), None);
    }

    // (host, blocked): live CPython 3.12 `ipaddress` verdicts, probed from
    // the 3.12 source tables plus boundary addresses.
    const BATTERY: &[(&str, bool)] = &[
        ("0.0.0.0", true),
        ("0.0.0.1", true),
        ("0.255.255.255", true),
        ("1.0.0.0", false),
        ("1.1.1.1", false),
        ("8.8.8.8", false),
        ("10.0.0.1", true),
        ("100.64.0.1", false),
        ("100.127.255.255", false),
        ("101.64.0.1", false),
        ("127.0.0.1", true),
        ("169.254.0.1", true),
        ("169.254.169.254", true),
        ("172.16.0.1", true),
        ("172.31.255.255", true),
        ("172.32.0.1", false),
        ("192.0.0.0", true),
        ("192.0.0.7", true),
        ("192.0.0.8", true),
        ("192.0.0.9", false),
        ("192.0.0.10", false),
        ("192.0.0.169", true),
        ("192.0.0.170", true),
        ("192.0.0.171", true),
        ("192.0.2.1", true),
        ("192.31.196.1", false),
        ("192.52.193.1", false),
        ("192.168.1.1", true),
        ("192.175.48.1", false),
        ("198.18.0.1", true),
        ("198.19.0.1", true),
        ("198.51.100.1", true),
        ("203.0.113.1", true),
        ("223.255.255.255", false),
        ("224.0.0.1", true),
        ("239.255.255.255", true),
        ("240.0.0.1", true),
        ("255.255.255.254", true),
        ("255.255.255.255", true),
        ("::", true),
        ("::1", true),
        ("::2", true),
        ("::3", true),
        ("::4", true),
        ("::ffff", true),
        ("::ffff:0.0.0.0", true),
        ("::ffff:0:1", true),
        ("::ffff:1.2.3.4", false),
        ("::ffff:8.8.8.8", false),
        ("::ffff:10.0.0.1", true),
        ("::ffff:100.64.0.1", false),
        ("::ffff:127.0.0.1", true),
        ("::ffff:192.168.0.1", true),
        ("1::1", true),
        ("1::2", true),
        ("2::1", true),
        ("5f00::1", true),
        ("5e00::1", true),
        ("5fff:ffff::1", true),
        ("64:ff9b::1", true),
        ("64:ff9b::808:808", true),
        ("64:ff9b:1::1", true),
        ("64:ff9c::1", true),
        ("100::1", true),
        ("100::2", true),
        ("100:0:0:1::1", true),
        ("2000::1", false),
        ("2001::1", true),
        ("2001:0::1", true),
        ("2001:1::1", false),
        ("2001:2::1", true),
        ("2001:4:112::1", false),
        ("2001:10::1", true),
        ("2001:30::1", false),
        ("2001:db8::1", true),
        ("2001:db8:ffff::1", true),
        ("2001:db8:1::1", true),
        ("2001:ff::1", true),
        ("2002:808:808::1", true),
        ("2003::1", false),
        ("2620:0::1", false),
        ("300::1", true),
        ("3fff::1", true),
        ("fc00::1", true),
        ("fcff:ffff::1", true),
        ("fd00::1", true),
        ("fdff:ffff::1", true),
        ("fe80::1", true),
        ("fe7f::1", true),
        ("fe90::1", true),
        ("fec0::1", false),
        ("ff00::1", true),
        ("ff01::1", true),
        ("ff02::1", true),
        ("ff05::1", true),
    ];

    #[test]
    fn address_battery_matches_cpython_312() {
        for (host, expected) in BATTERY {
            assert_eq!(address_is_blocked(host), *expected, "{host}");
        }
    }

    #[test]
    fn unparseable_resolved_address_is_blocked() {
        assert!(address_is_blocked("fe80::1%eth0"));
        assert!(address_is_blocked("not an ip"));
    }

    #[test]
    fn any_blocked_address_blocks_the_host() {
        let resolver = fake(&[("dual", vec!["8.8.8.8", "10.0.0.1"])]);
        assert!(is_blocked("http://dual/", true, &resolver));
        let resolver = fake(&[("clean", vec!["8.8.8.8", "1.1.1.1"])]);
        assert!(!is_blocked("http://clean/", true, &resolver));
    }

    #[test]
    fn system_resolver_needs_no_dns_for_numerics() {
        let system = SystemResolver;
        assert!(is_blocked("http://127.0.0.1/", true, &system));
        assert!(!is_blocked("http://8.8.8.8/", true, &system));
        assert!(is_blocked("http://[::1]/", true, &system));
        assert!(!is_blocked("http://8.8.8.8./", true, &system));
    }
}
