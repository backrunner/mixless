//! Let CFNetwork interpret macOS proxy exceptions and PAC rules for each URL.
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFType, TCFType},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
    url::{CFURLCreateWithString, CFURLRef, CFURL},
};
use std::{io::Read, ptr, time::Duration};

#[link(name = "CFNetwork", kind = "framework")]
extern "C" {
    fn CFNetworkCopySystemProxySettings() -> CFDictionaryRef;
    fn CFNetworkCopyProxiesForURL(url: CFURLRef, settings: CFDictionaryRef) -> CFArrayRef;
    fn CFNetworkCopyProxiesForAutoConfigurationScript(
        script: CFStringRef,
        url: CFURLRef,
        error: *mut *const std::ffi::c_void,
    ) -> CFArrayRef;
    static kCFProxyTypeKey: CFStringRef;
    static kCFProxyHostNameKey: CFStringRef;
    static kCFProxyPortNumberKey: CFStringRef;
    static kCFProxyAutoConfigurationURLKey: CFStringRef;
    static kCFProxyAutoConfigurationJavaScriptKey: CFStringRef;
    static kCFProxyTypeNone: CFStringRef;
    static kCFProxyTypeHTTP: CFStringRef;
    static kCFProxyTypeHTTPS: CFStringRef;
    static kCFProxyTypeSOCKS: CFStringRef;
    static kCFProxyTypeAutoConfigurationURL: CFStringRef;
    static kCFProxyTypeAutoConfigurationJavaScript: CFStringRef;
}

fn cf_url(url: &str) -> Result<CFURL, String> {
    let url = CFString::new(url);
    let result =
        unsafe { CFURLCreateWithString(ptr::null(), url.as_concrete_TypeRef(), ptr::null()) };
    if result.is_null() {
        return Err("Invalid proxy target URL".into());
    }
    Ok(unsafe { CFURL::wrap_under_create_rule(result) })
}

pub(super) fn resolve(url: &str) -> Result<Option<String>, String> {
    let target = cf_url(url)?;
    let settings = unsafe { CFNetworkCopySystemProxySettings() };
    if settings.is_null() {
        return Ok(None);
    }
    let settings: CFDictionary = unsafe { CFDictionary::wrap_under_create_rule(settings) };
    let proxies = unsafe {
        CFNetworkCopyProxiesForURL(target.as_concrete_TypeRef(), settings.as_concrete_TypeRef())
    };
    choose(proxies, &target, true)
}

fn evaluate(script: &str, target: &CFURL) -> Result<Option<String>, String> {
    let mut error = ptr::null();
    let script = CFString::new(script);
    let proxies = unsafe {
        CFNetworkCopyProxiesForAutoConfigurationScript(
            script.as_concrete_TypeRef(),
            target.as_concrete_TypeRef(),
            &mut error,
        )
    };
    if !error.is_null() {
        drop(unsafe { CFType::wrap_under_create_rule(error) });
    }
    choose(proxies, target, false)
}

fn choose(proxies: CFArrayRef, target: &CFURL, allow_pac: bool) -> Result<Option<String>, String> {
    if proxies.is_null() {
        return Err("Could not resolve the system proxy configuration".into());
    }
    let proxies: CFArray = unsafe { CFArray::wrap_under_create_rule(proxies) };
    let Some(first) = proxies.iter().next() else {
        return Err("The system proxy returned no route".into());
    };
    let first = unsafe { CFType::wrap_under_get_rule(*first) };
    let dictionary = first
        .downcast::<CFDictionary>()
        .ok_or("Invalid system proxy route")?;
    let get = |key: CFStringRef| {
        dictionary
            .find(key.cast())
            .map(|value| unsafe { CFType::wrap_under_get_rule(*value) })
    };
    let string = |key| {
        get(key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|v| v.to_string())
    };
    let kind = string(unsafe { kCFProxyTypeKey }).ok_or("Missing system proxy type")?;
    let is =
        |value: CFStringRef| kind == unsafe { CFString::wrap_under_get_rule(value) }.to_string();
    if is(unsafe { kCFProxyTypeNone }) {
        return Ok(None);
    }
    if is(unsafe { kCFProxyTypeAutoConfigurationURL })
        || is(unsafe { kCFProxyTypeAutoConfigurationJavaScript })
    {
        if !allow_pac {
            return Err("Recursive automatic proxy configuration".into());
        }
        let script = if let Some(script) = string(unsafe { kCFProxyAutoConfigurationJavaScriptKey })
        {
            script
        } else {
            let url = get(unsafe { kCFProxyAutoConfigurationURLKey })
                .and_then(|v| v.downcast::<CFURL>())
                .ok_or("Missing automatic proxy URL")?
                .get_string()
                .to_string();
            // Fetch the routing script directly to avoid asking that script
            // how to fetch itself. Do not send any provider credentials.
            let parsed = url::Url::parse(&url).map_err(|_| "Invalid automatic proxy URL")?;
            let mut bytes = Vec::new();
            if parsed.scheme() == "file" {
                std::fs::File::open(
                    parsed
                        .to_file_path()
                        .map_err(|_| "Invalid local proxy script")?,
                )
                .map_err(|_| "Cannot read the automatic proxy script")?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read the automatic proxy script")?;
            } else if matches!(parsed.scheme(), "http" | "https") {
                ureq::AgentBuilder::new()
                    .try_proxy_from_env(false)
                    .timeout(Duration::from_secs(10))
                    .build()
                    .get(&url)
                    .call()
                    .map_err(|_| "Cannot download the automatic proxy script")?
                    .into_reader()
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Cannot read the automatic proxy script")?;
            } else {
                return Err("Unsupported automatic proxy URL".into());
            }
            if bytes.len() > 1024 * 1024 {
                return Err("Automatic proxy script is too large".into());
            }
            String::from_utf8(bytes).map_err(|_| "Invalid automatic proxy script")?
        };
        return evaluate(&script, target);
    }
    let transport = if is(unsafe { kCFProxyTypeHTTP }) || is(unsafe { kCFProxyTypeHTTPS }) {
        "http"
    } else if is(unsafe { kCFProxyTypeSOCKS }) {
        "socks5"
    } else {
        return Err("Unsupported system proxy protocol".into());
    };
    let host = string(unsafe { kCFProxyHostNameKey }).ok_or("Missing system proxy host")?;
    let port = get(unsafe { kCFProxyPortNumberKey })
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|v| v.to_i64())
        .filter(|p| (1..=65535).contains(p))
        .ok_or("Invalid system proxy port")?;
    Ok(Some(format!("{transport}://{host}:{port}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pac_routes_each_host_and_respects_direct() {
        let script = "function FindProxyForURL(url, host) { return host == 'music.youtube.com' ? 'PROXY 127.0.0.1:7890' : 'DIRECT'; }";
        assert_eq!(
            evaluate(script, &cf_url("https://music.youtube.com/search").unwrap()).unwrap(),
            Some("http://127.0.0.1:7890".into())
        );
        assert_eq!(
            evaluate(script, &cf_url("https://open.spotify.com/").unwrap()).unwrap(),
            None
        );
    }
}
