pub fn default_hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    if let Ok(h) = std::env::var("COMPUTERNAME")
        && !h.is_empty()
    {
        return h;
    }
    platform_hostname().unwrap_or_else(|| "runner".to_string())
}

#[cfg(unix)]
fn platform_hostname() -> Option<String> {
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
}

#[cfg(not(unix))]
fn platform_hostname() -> Option<String> {
    None
}
