//! Shared routing for metadata requests and downloader child processes.
//! Resolve on each operation so changing the system proxy needs no app restart.
use std::time::Duration;

pub fn proxy_for(url: &str) -> Result<Option<String>, String> {
    let url = url::Url::parse(url).map_err(|_| "Invalid network URL")?;
    let host = url.host_str().ok_or("Missing network host")?;
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    if env("no_proxy")
        .or_else(|| env("NO_PROXY"))
        .is_some_and(|list| list.split(',').any(|entry| bypass(host, entry.trim())))
    {
        return Ok(None);
    }
    // Explicit environment overrides remain useful for command-line launches.
    let scheme = url.scheme();
    if let Some(proxy) = env(&format!("{scheme}_proxy"))
        .or_else(|| env(&format!("{}_PROXY", scheme.to_ascii_uppercase())))
        .or_else(|| env("all_proxy"))
        .or_else(|| env("ALL_PROXY"))
    {
        return Ok(Some(proxy));
    }
    system_proxy(&url)
}

pub fn agent(url: &str, timeout: Duration) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout(timeout)
        .try_proxy_from_env(false);
    if let Some(proxy) = proxy_for(url)? {
        builder = builder.proxy(
            ureq::Proxy::new(proxy.replace("socks5h://", "socks5://"))
                .map_err(|_| "The configured proxy is not supported")?,
        );
    }
    Ok(builder.build())
}

fn bypass(host: &str, entry: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let entry = entry.to_ascii_lowercase();
    if entry == "*" || (entry == "<local>" && !host.contains('.')) {
        return true;
    }
    if let (Ok(net), Ok(ip)) = (
        entry.parse::<ipnet::IpNet>(),
        host.parse::<std::net::IpAddr>(),
    ) {
        return net.contains(&ip);
    }
    let domain = entry.trim_start_matches("*.").trim_start_matches('.');
    !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

#[cfg(not(target_os = "macos"))]
fn system_proxy(_url: &url::Url) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
fn system_proxy(url: &url::Url) -> Result<Option<String>, String> {
    macos::resolve(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proxy_exceptions_respect_domain_boundaries_and_networks() {
        for (host, rule) in [
            ("music.youtube.com", "*.youtube.com"),
            ("youtube.com", ".youtube.com"),
            ("169.254.1.2", "169.254.0.0/16"),
            ("printer", "<local>"),
            ("example.com", "*"),
        ] {
            assert!(bypass(host, rule));
        }
        assert!(!bypass("notyoutube.com", ".youtube.com"));
        assert!(!bypass("youtube.com.evil.test", "youtube.com"));
        assert!(!bypass("10.0.0.1", "169.254.0.0/16"));
    }
}
