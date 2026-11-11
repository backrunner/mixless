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
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .proxy(None);
    if let Some(proxy) = proxy_for(url)? {
        builder = builder.proxy(Some(
            ureq::Proxy::new(&proxy.replace("socks5h://", "socks5://"))
                .map_err(|_| "The configured proxy is not supported")?,
        ));
    }
    Ok(builder.build().into())
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
    fn status_errors_keep_retry_headers_and_response_body() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(connection.read(&mut request).unwrap() > 0);
            connection.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 17\r\nContent-Length: 4\r\nConnection: close\r\n\r\nwait").unwrap();
        });
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(5)))
            .http_status_as_error(false)
            .build()
            .into();
        let response = agent.get(format!("http://{address}/")).call().unwrap();
        let HttpError::Status(429, response) = check_response(response).unwrap_err() else {
            panic!("expected rate limit status");
        };
        assert_eq!(response.headers().get("Retry-After").unwrap(), "17");
        assert_eq!(response.into_body().read_to_string().unwrap(), "wait");
        server.join().unwrap();
    }
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

/// Preserve HTTP headers for rate-limit handling; ureq 3's status-only errors
/// otherwise discard Retry-After along with the response.
#[derive(Debug)]
pub enum HttpError {
    Transport(ureq::Error),
    Status(u16, Box<ureq::http::Response<ureq::Body>>),
}
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => e.fmt(f),
            Self::Status(status, _) => write!(f, "HTTP status {status}"),
        }
    }
}
impl std::error::Error for HttpError {}

pub fn check_response(
    response: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, HttpError> {
    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        Err(HttpError::Status(status.as_u16(), Box::new(response)))
    } else {
        Ok(response)
    }
}
