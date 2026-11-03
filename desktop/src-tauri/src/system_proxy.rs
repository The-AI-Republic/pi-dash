// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! reqwest's `system-proxy` takes the proxy address from the OS but not the
//! list of hosts the user told the OS to reach directly: on macOS it never
//! reads the exceptions list, and on Windows it rewrites `ProxyOverride` into
//! `NO_PROXY` syntax, which loses `<local>` and wildcards. A server on a
//! private network is then sent to a proxy that cannot reach it, while the
//! user's browser works.
//!
//! So when the OS has a proxy configured we decide per request ourselves:
//! hosts on the bypass list connect directly, everything else keeps going
//! through the proxy. Linux has no system-wide setting that reqwest reads
//! (only the `*_PROXY` variables, where `NO_PROXY` already works), so the
//! client is left alone there.

use std::net::IpAddr;

use reqwest::{ClientBuilder, Proxy, Url};

/// Apply the OS proxy settings, bypass list included, to a client. Used for
/// every native client that talks to the server (API requests, the updater).
pub fn configure(builder: ClientBuilder) -> ClientBuilder {
    match Rules::resolve(&Env::read(), read_system()) {
        Some(rules) => builder.proxy(rules.into_proxy()),
        // No OS proxy: reqwest's own handling of the environment is correct.
        None => builder,
    }
}

/// The OS proxy settings, before the environment is taken into account.
#[derive(Debug, Default, PartialEq)]
struct SystemSettings {
    http: Option<String>,
    https: Option<String>,
    bypass: Vec<String>,
    // macOS "Exclude simple hostnames"; Windows spells it `<local>` in the
    // bypass list instead.
    bypass_simple_hostnames: bool,
}

#[derive(Debug, Default)]
struct Env {
    http: Option<String>,
    https: Option<String>,
    all: Option<String>,
    no: Option<String>,
}

impl Env {
    fn read() -> Self {
        let first = |names: [&str; 2]| {
            names
                .iter()
                .filter_map(|name| std::env::var(name).ok())
                .find(|value| !value.trim().is_empty())
        };
        Self {
            http: first(["HTTP_PROXY", "http_proxy"]),
            https: first(["HTTPS_PROXY", "https_proxy"]),
            all: first(["ALL_PROXY", "all_proxy"]),
            no: first(["NO_PROXY", "no_proxy"]),
        }
    }
}

#[derive(Debug)]
struct Rules {
    http: Option<String>,
    https: Option<String>,
    bypass: Bypass,
}

impl Rules {
    /// `None` when the OS has no proxy configured. Otherwise the environment
    /// keeps the precedence reqwest gives it: a scheme's own variable, then
    /// the OS proxy, then `ALL_PROXY`. A host is direct when either the OS
    /// list or `NO_PROXY` says so, so launching from a shell that exports
    /// `NO_PROXY` does not drop the OS exceptions.
    fn resolve(env: &Env, system: SystemSettings) -> Option<Self> {
        let system_http = system.http.as_deref().and_then(proxy_url);
        let system_https = system.https.as_deref().and_then(proxy_url);
        if system_http.is_none() && system_https.is_none() {
            return None;
        }
        let from_env = |value: &Option<String>| value.as_deref().and_then(proxy_url);
        let all = from_env(&env.all);
        let mut bypass = Bypass::new(&system.bypass, system.bypass_simple_hostnames);
        bypass.add_no_proxy(env.no.as_deref().unwrap_or_default());
        Some(Self {
            http: from_env(&env.http).or(system_http).or_else(|| all.clone()),
            https: from_env(&env.https).or(system_https).or(all),
            bypass,
        })
    }

    /// The proxy to use for `url`, or `None` to connect directly.
    fn proxy_for(&self, url: &Url) -> Option<String> {
        let proxy = match url.scheme() {
            "http" => self.http.as_ref(),
            "https" => self.https.as_ref(),
            _ => None,
        }?;
        if self.bypass.matches(url.host_str()?) {
            return None;
        }
        Some(proxy.clone())
    }

    fn into_proxy(self) -> Proxy {
        Proxy::custom(move |url| self.proxy_for(url))
    }
}

/// OS settings give `host:port`; the environment usually gives a full URL.
fn proxy_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    Url::parse(&value).ok().map(String::from)
}

/// The hosts that must not go through the proxy.
///
/// Entries follow what the OS settings and browsers accept:
/// - `example.com` matches that host only,
/// - `*.example.com` and `.example.com` match its subdomains, and `*` may
///   appear anywhere (`192.168.*`, `*`),
/// - `172.16.0.0/12` (or the macOS short form `169.254/16`) matches hosts
///   given as an IP address in that range. A hostname is never resolved to
///   test it against a range, as in browsers.
#[derive(Debug, Default)]
struct Bypass {
    rules: Vec<Rule>,
    simple_hostnames: bool,
}

#[derive(Debug, PartialEq)]
enum Rule {
    // Lowercase, `*` is the only wildcard.
    Host(String),
    Network(IpAddr, u8),
}

impl Bypass {
    fn new<S: AsRef<str>>(entries: &[S], simple_hostnames: bool) -> Self {
        let mut bypass = Self {
            rules: Vec::new(),
            simple_hostnames,
        };
        for entry in entries {
            let entry = entry.as_ref().trim().to_ascii_lowercase();
            if entry.is_empty() {
                continue;
            }
            if entry == "<local>" {
                bypass.simple_hostnames = true;
            } else if let Some(network) = parse_network(&entry) {
                bypass.rules.push(network);
            } else if let Some(suffix) = entry.strip_prefix('.') {
                bypass.rules.push(Rule::Host(format!("*.{suffix}")));
            } else {
                bypass.rules.push(Rule::Host(entry));
            }
        }
        bypass
    }

    /// `NO_PROXY` is comma-separated and, unlike the OS lists, a bare domain
    /// also covers its subdomains.
    fn add_no_proxy(&mut self, value: &str) {
        for entry in value.split(',') {
            let entry = entry.trim().to_ascii_lowercase();
            if entry.is_empty() {
                continue;
            }
            if let Some(network) = parse_network(&entry) {
                self.rules.push(network);
            } else if entry == "*" {
                self.rules.push(Rule::Host(entry));
            } else {
                let domain = entry.trim_start_matches('.');
                self.rules.push(Rule::Host(format!("*.{domain}")));
                self.rules.push(Rule::Host(domain.to_owned()));
            }
        }
    }

    fn matches(&self, host: &str) -> bool {
        // Url::host_str brackets IPv6 addresses.
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let ip = host.parse::<IpAddr>().ok();
        if ip.is_none() && self.simple_hostnames && !host.contains('.') {
            return true;
        }
        self.rules.iter().any(|rule| match rule {
            Rule::Host(pattern) => glob(pattern.as_bytes(), host.as_bytes()),
            Rule::Network(network, prefix) => {
                ip.is_some_and(|ip| in_network(ip, *network, *prefix))
            }
        })
    }
}

/// An IP address or CIDR range. macOS writes IPv4 ranges without the trailing
/// zero octets (`169.254/16`).
fn parse_network(entry: &str) -> Option<Rule> {
    let entry = entry.trim_start_matches('[').trim_end_matches(']');
    let Some((address, prefix)) = entry.split_once('/') else {
        let address = entry.parse::<IpAddr>().ok()?;
        let bits = if address.is_ipv4() { 32 } else { 128 };
        return Some(Rule::Network(address, bits));
    };
    let prefix = prefix.parse::<u8>().ok()?;
    let address = address.parse::<IpAddr>().ok().or_else(|| {
        let mut octets = [0u8; 4];
        let parts: Vec<&str> = address.split('.').collect();
        if parts.len() > 4 {
            return None;
        }
        for (octet, part) in octets.iter_mut().zip(parts) {
            *octet = part.parse().ok()?;
        }
        Some(IpAddr::from(octets))
    })?;
    let bits = if address.is_ipv4() { 32 } else { 128 };
    (prefix <= bits).then_some(Rule::Network(address, prefix))
}

fn in_network(ip: IpAddr, network: IpAddr, prefix: u8) -> bool {
    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(network)) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
            u32::from(ip) & mask == u32::from(network) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(network)) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
            u128::from(ip) & mask == u128::from(network) & mask
        }
        _ => false,
    }
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| glob(rest, &text[skip..])),
        Some((byte, rest)) => text.first() == Some(byte) && glob(rest, &text[1..]),
    }
}

#[cfg(target_os = "macos")]
fn read_system() -> SystemSettings {
    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::base::{CFType, CFTypeRef, TCFType};
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesExcludeSimpleHostnames,
        kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPPort, kSCPropNetProxiesHTTPProxy,
        kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort, kSCPropNetProxiesHTTPSProxy,
    };

    let Some(proxies) = SCDynamicStoreBuilder::new("pi-dash-desktop")
        .build()
        .and_then(|store| store.get_proxies())
    else {
        return SystemSettings::default();
    };
    let number = |key: CFStringRef| {
        proxies
            .find(key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i32())
    };
    let string = |key: CFStringRef| {
        proxies
            .find(key)
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
    };
    let proxy = |enable: CFStringRef, host: CFStringRef, port: CFStringRef| {
        if number(enable) != Some(1) {
            return None;
        }
        let host = string(host)?;
        Some(match number(port) {
            Some(port) => format!("{host}:{port}"),
            None => host,
        })
    };
    // SAFETY: the schema keys are immutable CFString constants exported by
    // SystemConfiguration, and the array items are owned by `proxies`, which
    // outlives every reference taken here.
    unsafe {
        SystemSettings {
            http: proxy(
                kSCPropNetProxiesHTTPEnable,
                kSCPropNetProxiesHTTPProxy,
                kSCPropNetProxiesHTTPPort,
            ),
            https: proxy(
                kSCPropNetProxiesHTTPSEnable,
                kSCPropNetProxiesHTTPSProxy,
                kSCPropNetProxiesHTTPSPort,
            ),
            bypass: proxies
                .find(kSCPropNetProxiesExceptionsList)
                .and_then(|value| value.downcast::<CFArray>())
                .map(|list| {
                    list.iter()
                        .filter_map(|item| {
                            CFType::wrap_under_get_rule(*item as CFTypeRef).downcast::<CFString>()
                        })
                        .map(|entry| entry.to_string())
                        .collect()
                })
                .unwrap_or_default(),
            bypass_simple_hostnames: number(kSCPropNetProxiesExcludeSimpleHostnames) == Some(1),
        }
    }
}

#[cfg(windows)]
fn read_system() -> SystemSettings {
    let Ok(settings) = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
    else {
        return SystemSettings::default();
    };
    if settings.get_u32("ProxyEnable").unwrap_or(0) == 0 {
        return SystemSettings::default();
    }
    let (http, https) =
        windows_proxy_server(&settings.get_string("ProxyServer").unwrap_or_default());
    SystemSettings {
        http,
        https,
        bypass: settings
            .get_string("ProxyOverride")
            .unwrap_or_default()
            .split(';')
            .map(str::to_owned)
            .collect(),
        bypass_simple_hostnames: false,
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read_system() -> SystemSettings {
    SystemSettings::default()
}

/// `ProxyServer` is either one `host:port` for every protocol or a list such
/// as `http=proxy:80;https=proxy:443`.
#[cfg(any(windows, test))]
fn windows_proxy_server(value: &str) -> (Option<String>, Option<String>) {
    if !value.contains('=') {
        let proxy = Some(value.trim().to_owned()).filter(|proxy| !proxy.is_empty());
        return (proxy.clone(), proxy);
    }
    let scheme = |name: &str| {
        value
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(scheme, _)| scheme.trim().eq_ignore_ascii_case(name))
            .map(|(_, proxy)| proxy.trim().to_owned())
    };
    (scheme("http"), scheme("https"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn bypass(entries: &[&str]) -> Bypass {
        Bypass::new(entries, false)
    }

    fn system(bypass: &[&str]) -> SystemSettings {
        SystemSettings {
            http: Some("127.0.0.1:1082".into()),
            https: Some("127.0.0.1:1082".into()),
            bypass: bypass.iter().map(|entry| (*entry).to_owned()).collect(),
            bypass_simple_hostnames: false,
        }
    }

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn wildcard_entries_match_subdomains_only() {
        let list = bypass(&["*.example.com", ".internal.test"]);
        assert!(list.matches("api.example.com"));
        assert!(list.matches("a.b.EXAMPLE.com"));
        assert!(list.matches("api.example.com."));
        assert!(list.matches("git.internal.test"));
        assert!(!list.matches("example.com"));
        assert!(!list.matches("notexample.com"));
        assert!(!list.matches("example.com.evil.test"));
    }

    #[test]
    fn plain_entries_match_one_host() {
        let list = bypass(&["example.com", " Intranet "]);
        assert!(list.matches("example.com"));
        assert!(list.matches("intranet"));
        assert!(!list.matches("api.example.com"));
        assert!(bypass(&["*"]).matches("anything.test"));
        assert!(!bypass(&[]).matches("example.com"));
    }

    #[test]
    fn cidr_entries_match_ip_hosts_in_range() {
        let list = bypass(&["172.16.0.0/12", "169.254/16", "fd00::/8", "10.1.2.3"]);
        assert!(list.matches("172.16.0.1"));
        assert!(list.matches("172.31.255.254"));
        assert!(!list.matches("172.32.0.1"));
        assert!(!list.matches("172.15.255.255"));
        assert!(list.matches("169.254.10.20"));
        assert!(!list.matches("169.253.0.1"));
        assert!(list.matches("[fd12:3456::1]"));
        assert!(!list.matches("[fe80::1]"));
        assert!(list.matches("10.1.2.3"));
        assert!(!list.matches("10.1.2.4"));
        // A hostname is not resolved to test it against a range.
        assert!(!list.matches("api.example.com"));
        // An IPv4 range never covers an IPv6 host.
        assert!(!bypass(&["0.0.0.0/0"]).matches("[::1]"));
        assert!(bypass(&["0.0.0.0/0"]).matches("8.8.8.8"));
    }

    #[test]
    fn ip_wildcards_and_simple_hostnames() {
        let list = bypass(&["192.168.*", "<local>"]);
        assert!(list.matches("192.168.1.20"));
        assert!(!list.matches("192.169.1.20"));
        assert!(list.matches("buildbox"));
        assert!(!list.matches("buildbox.example.com"));
        // macOS reports the same setting as a flag instead of an entry.
        assert!(Bypass::new::<&str>(&[], true).matches("buildbox"));
        assert!(!Bypass::new::<&str>(&[], true).matches("10.0.0.1"));
    }

    #[test]
    fn bypassed_hosts_connect_directly_and_the_rest_use_the_proxy() {
        let rules = Rules::resolve(
            &Env::default(),
            system(&["*.airepublic.com", "172.16.0.0/12"]),
        )
        .unwrap();
        assert_eq!(
            rules.proxy_for(&url("https://apitestpidash.airepublic.com/api/instances/")),
            None
        );
        assert_eq!(
            rules.proxy_for(&url("https://172.31.4.5/api/instances/")),
            None
        );
        assert_eq!(
            rules.proxy_for(&url("https://pidash.example.com/api/instances/")),
            Some("http://127.0.0.1:1082/".into())
        );
        assert_eq!(
            rules.proxy_for(&url("http://pidash.example.com/")),
            Some("http://127.0.0.1:1082/".into())
        );
    }

    #[test]
    fn without_an_os_proxy_the_client_is_left_to_reqwest() {
        let settings = SystemSettings {
            bypass: vec!["*.example.com".into()],
            ..Default::default()
        };
        let env = Env {
            https: Some("http://env-proxy:3128".into()),
            ..Default::default()
        };
        assert!(Rules::resolve(&env, settings).is_none());
    }

    #[test]
    fn environment_overrides_the_os_proxy_and_adds_to_the_bypass_list() {
        let env = Env {
            https: Some("http://user:pass@env-proxy:3128".into()),
            all: Some("http://all-proxy:8080".into()),
            no: Some("corp.test, .lab.test,10.0.0.0/8".into()),
            ..Default::default()
        };
        let rules = Rules::resolve(&env, system(&["*.airepublic.com"])).unwrap();
        // The scheme's own variable wins; the OS proxy beats ALL_PROXY.
        assert_eq!(
            rules.proxy_for(&url("https://pidash.example.com/")),
            Some("http://user:pass@env-proxy:3128/".into())
        );
        assert_eq!(
            rules.proxy_for(&url("http://pidash.example.com/")),
            Some("http://127.0.0.1:1082/".into())
        );
        // NO_PROXY keeps its meaning (a domain covers its subdomains) and the
        // OS list still applies.
        for direct in [
            "https://corp.test/",
            "https://api.corp.test/",
            "https://lab.test/",
            "https://10.9.8.7/",
            "https://api.airepublic.com/",
        ] {
            assert_eq!(rules.proxy_for(&url(direct)), None, "{direct}");
        }
        assert!(rules.proxy_for(&url("https://notcorp.test/")).is_some());
    }

    #[test]
    fn all_proxy_fills_a_scheme_the_os_does_not_proxy() {
        let settings = SystemSettings {
            https: Some("secure-proxy:443".into()),
            ..Default::default()
        };
        let env = Env {
            all: Some("http://all-proxy:8080".into()),
            ..Default::default()
        };
        let rules = Rules::resolve(&env, settings).unwrap();
        assert_eq!(
            rules.proxy_for(&url("http://pidash.example.com/")),
            Some("http://all-proxy:8080/".into())
        );
        assert_eq!(
            rules.proxy_for(&url("https://pidash.example.com/")),
            Some("http://secure-proxy:443/".into())
        );
    }

    #[test]
    fn windows_proxy_server_forms() {
        assert_eq!(
            windows_proxy_server("proxy.corp:8080"),
            (
                Some("proxy.corp:8080".into()),
                Some("proxy.corp:8080".into())
            )
        );
        assert_eq!(
            windows_proxy_server("http=web:80; HTTPS=secure:443;ftp=files:21"),
            (Some("web:80".into()), Some("secure:443".into()))
        );
        assert_eq!(windows_proxy_server("socks=socks:1080"), (None, None));
        assert_eq!(windows_proxy_server(""), (None, None));
    }

    /// Answer every connection with `status`, counting the requests.
    async fn serve(
        status: &'static str,
    ) -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await;
                let _ = socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await;
            }
        });
        (address, hits)
    }

    // The reported failure, end to end: a proxy that cannot reach the server.
    #[tokio::test]
    async fn client_skips_a_failing_proxy_only_for_bypassed_hosts() {
        // As DesktopHttp::new does; this crate enables no default provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (server, server_hits) = serve("200 OK").await;
        let (proxy, proxy_hits) = serve("503 Service Unavailable").await;
        let settings = |bypass: &[&str]| SystemSettings {
            http: Some(proxy.to_string()),
            bypass: bypass.iter().map(|entry| (*entry).to_owned()).collect(),
            ..Default::default()
        };
        let client = |bypass: &[&str]| {
            let rules = Rules::resolve(&Env::default(), settings(bypass)).unwrap();
            reqwest::Client::builder()
                .proxy(rules.into_proxy())
                .build()
                .unwrap()
        };
        let target = format!("http://{server}/api/instances/");

        let response = client(&[]).get(&target).send().await.unwrap();
        assert_eq!(response.status(), 503);
        assert_eq!(proxy_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(server_hits.load(std::sync::atomic::Ordering::SeqCst), 0);

        let response = client(&["127.0.0.0/8"]).get(&target).send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(proxy_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(server_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
