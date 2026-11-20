//! The host API available to plugins as the global `veena` object.
//!
//! Each plugin runs on its own thread, so per-plugin host state lives in a
//! thread-local and the native functions are plain function pointers.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use boa_engine::object::ObjectInitializer;
use boa_engine::object::builtins::JsPromise;
use boa_engine::property::Attribute;
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, NativeFunction, Source, js_string,
};
use serde::Deserialize;
use serde_json::{Value, json};
use veena_net::ureq::{self, Agent, http};

use crate::Limits;

const MAX_REDIRECTS: usize = 5;
const MAX_CACHE_ENTRIES: usize = 1000;

/// JavaScript conveniences layered over the native functions.
const PRELUDE: &str = r#"
(() => {
  const nativeFetch = veena.http.fetch;
  veena.http.fetch = async (url, options) => {
    const response = await nativeFetch(String(url), options ?? {});
    response.json = () => JSON.parse(response.text);
    return response;
  };
  const write = veena.__log;
  delete veena.__log;
  const format = (args) => args
    .map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
    .join(" ");
  veena.log = (...args) => write("info", format(args));
  globalThis.console = Object.freeze({
    log: (...args) => write("info", format(args)),
    info: (...args) => write("info", format(args)),
    debug: (...args) => write("debug", format(args)),
    warn: (...args) => write("warn", format(args)),
    error: (...args) => write("error", format(args)),
  });
  Object.freeze(veena.http);
  Object.freeze(veena.cache);
  Object.freeze(veena);
})();
"#;

pub struct HostState {
    pub plugin: String,
    /// Allowed hosts. Empty while the module loads, so top-level code
    /// cannot make requests.
    pub domains: Vec<String>,
    pub limits: Limits,
    /// Requests made during the current call.
    pub requests: u32,
    cache: HashMap<String, (String, Option<Instant>)>,
}

impl HostState {
    pub fn new(plugin: String, limits: Limits) -> Self {
        Self {
            plugin,
            domains: Vec::new(),
            limits,
            requests: 0,
            cache: HashMap::new(),
        }
    }
}

thread_local! {
    pub static HOST: RefCell<Option<HostState>> = const { RefCell::new(None) };
}

fn with_host<T>(f: impl FnOnce(&mut HostState) -> T) -> T {
    HOST.with(|host| f(host.borrow_mut().as_mut().expect("host state is installed")))
}

/// Define the `veena` global and run the prelude.
pub fn install(context: &mut Context, config: &serde_json::Map<String, Value>) -> JsResult<()> {
    let http = ObjectInitializer::new(context)
        .function(NativeFunction::from_fn_ptr(fetch), js_string!("fetch"), 2)
        .build();
    let cache = ObjectInitializer::new(context)
        .function(NativeFunction::from_fn_ptr(cache_get), js_string!("get"), 1)
        .function(NativeFunction::from_fn_ptr(cache_set), js_string!("set"), 3)
        .function(
            NativeFunction::from_fn_ptr(cache_delete),
            js_string!("delete"),
            1,
        )
        .build();
    let config = JsValue::from_json(&Value::Object(config.clone()), context)?;
    let veena = ObjectInitializer::new(context)
        .property(
            js_string!("apiVersion"),
            crate::API_VERSION,
            Attribute::READONLY,
        )
        .property(js_string!("http"), http, Attribute::READONLY)
        .property(js_string!("cache"), cache, Attribute::READONLY)
        .property(js_string!("config"), config, Attribute::READONLY)
        .function(NativeFunction::from_fn_ptr(log), js_string!("__log"), 2)
        .build();
    context.register_global_property(js_string!("veena"), veena, Attribute::READONLY)?;
    context.eval(Source::from_bytes(PRELUDE))?;
    Ok(())
}

fn arg_string(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    Ok(args
        .get(index)
        .cloned()
        .unwrap_or_default()
        .to_string(context)?
        .to_std_string_escaped())
}

fn log(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let level = arg_string(args, 0, context)?;
    let message = arg_string(args, 1, context)?;
    let plugin = with_host(|h| h.plugin.clone());
    let level = match level.as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "debug" => log::Level::Debug,
        _ => log::Level::Info,
    };
    log::log!(target: "veena_plugin", level, "[{plugin}] {message}");
    Ok(JsValue::undefined())
}

fn cache_get(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let key = arg_string(args, 0, context)?;
    let value = with_host(|h| match h.cache.get(&key) {
        Some((_, Some(expires))) if *expires <= Instant::now() => {
            h.cache.remove(&key);
            None
        }
        Some((value, _)) => Some(value.clone()),
        None => None,
    });
    Ok(value.map_or_else(JsValue::undefined, |v| js_string!(v).into()))
}

fn cache_set(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let key = arg_string(args, 0, context)?;
    let value = arg_string(args, 1, context)?;
    let ttl = match args.get(2) {
        Some(v) if !v.is_undefined() && !v.is_null() => {
            let secs = v.to_number(context)?;
            (secs.is_finite() && secs > 0.0).then(|| Duration::from_secs_f64(secs))
        }
        _ => None,
    };
    with_host(|h| {
        if h.cache.len() >= MAX_CACHE_ENTRIES && !h.cache.contains_key(&key) {
            let now = Instant::now();
            h.cache
                .retain(|_, (_, expires)| expires.is_none_or(|e| e > now));
            if h.cache.len() >= MAX_CACHE_ENTRIES {
                h.cache.clear();
            }
        }
        h.cache
            .insert(key, (value, ttl.map(|t| Instant::now() + t)));
    });
    Ok(JsValue::undefined())
}

fn cache_delete(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let key = arg_string(args, 0, context)?;
    with_host(|h| h.cache.remove(&key));
    Ok(JsValue::undefined())
}

#[derive(Deserialize, Default)]
struct FetchOptions {
    method: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
}

/// `veena.http.fetch(url, options)`: returns a promise of
/// `{ status, ok, url, headers, text }`.
///
/// The request runs synchronously on the plugin's thread; the promise is
/// already settled when returned, so plugins simply `await` it.
fn fetch(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 0, context)?;
    let options = match args.get(1).and_then(|v| v.to_json(context).transpose()) {
        Some(json) => serde_json::from_value(json?).map_err(|e| {
            JsError::from(JsNativeError::typ().with_message(format!("invalid fetch options: {e}")))
        })?,
        None => FetchOptions::default(),
    };
    let promise = match with_host(|h| request(h, &url, options)) {
        Ok(response) => JsPromise::resolve(JsValue::from_json(&response, context)?, context),
        Err(message) => JsPromise::reject(JsNativeError::typ().with_message(message), context),
    };
    Ok(promise.into())
}

/// Shared client for plugin requests. Redirects are followed manually so
/// every hop can be checked against the allowed domains.
fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        veena_net::config()
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into()
    })
}

fn request(host: &mut HostState, url: &str, options: FetchOptions) -> Result<Value, String> {
    host.requests += 1;
    if host.requests > host.limits.requests_per_call {
        return Err(format!(
            "request limit reached ({} per call)",
            host.limits.requests_per_call
        ));
    }

    let mut method = options
        .method
        .as_deref()
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let mut body = options.body;
    let mut url = url.to_owned();
    let agent = agent();
    let timeout = host.limits.request_timeout;

    for _ in 0..=MAX_REDIRECTS {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("only http and https URLs are allowed: {url}"));
        }
        let uri: http::Uri = url.parse().map_err(|_| format!("invalid URL: {url}"))?;
        check_allowed(&uri, &host.domains)?;

        let result = match (method.as_str(), &body) {
            ("GET", _) => prepare(agent.get(&url), &options.headers, timeout).call(),
            ("HEAD", _) => prepare(agent.head(&url), &options.headers, timeout).call(),
            ("POST", Some(body)) => {
                prepare(agent.post(&url), &options.headers, timeout).send(body.as_str())
            }
            ("POST", None) => prepare(agent.post(&url), &options.headers, timeout).send_empty(),
            (other, _) => return Err(format!("unsupported method {other}")),
        };
        let mut response = result.map_err(|e| format!("request to {url} failed: {e}"))?;
        let status = response.status().as_u16();

        if (300..400).contains(&status)
            && let Some(location) = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
        {
            url = join_url(&uri, location)?;
            if matches!(status, 301..=303) {
                method = "GET".into();
                body = None;
            }
            continue;
        }

        let mut headers = BTreeMap::<String, String>::new();
        for (name, value) in response.headers() {
            let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
            headers
                .entry(name.as_str().to_owned())
                .and_modify(|existing| {
                    existing.push_str(", ");
                    existing.push_str(&value);
                })
                .or_insert(value);
        }

        let limit = host.limits.max_response_bytes;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("reading response from {url} failed: {e}"))?;
        if bytes.len() as u64 > limit {
            return Err(format!("response from {url} is larger than {limit} bytes"));
        }

        return Ok(json!({
            "status": status,
            "ok": (200..300).contains(&status),
            "url": url,
            "headers": headers,
            "text": String::from_utf8_lossy(&bytes),
        }));
    }
    Err(format!("too many redirects from {url}"))
}

fn prepare<B>(
    mut request: ureq::RequestBuilder<B>,
    headers: &BTreeMap<String, String>,
    timeout: Duration,
) -> ureq::RequestBuilder<B> {
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    request.config().timeout_global(Some(timeout)).build()
}

/// Resolve a redirect target against the URL that returned it.
fn join_url(base: &http::Uri, location: &str) -> Result<String, String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    let scheme = base.scheme_str().unwrap_or("https");
    let authority = base.authority().map(|a| a.as_str()).unwrap_or_default();
    if let Some(rest) = location.strip_prefix("//") {
        Ok(format!("{scheme}://{rest}"))
    } else if location.starts_with('/') {
        Ok(format!("{scheme}://{authority}{location}"))
    } else {
        Err(format!("unsupported redirect target: {location}"))
    }
}

/// Reject URLs that are not http(s) or whose host is not allowed.
pub fn check_allowed(uri: &http::Uri, domains: &[String]) -> Result<(), String> {
    if !matches!(uri.scheme_str(), Some("https" | "http")) {
        return Err(format!("only http and https URLs are allowed: {uri}"));
    }
    let host = uri
        .host()
        .ok_or_else(|| format!("URL has no host: {uri}"))?
        .to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    let allowed = domains.iter().any(|domain| {
        let domain = domain.to_ascii_lowercase();
        host == domain
            || host
                .strip_suffix(domain.as_str())
                .is_some_and(|prefix| prefix.ends_with('.'))
    });
    if allowed {
        Ok(())
    } else {
        Err(format!("plugin is not allowed to contact {host}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(url: &str, domains: &[&str]) -> bool {
        let domains: Vec<String> = domains.iter().map(|d| d.to_string()).collect();
        check_allowed(&url.parse().unwrap(), &domains).is_ok()
    }

    #[test]
    fn domain_rules() {
        assert!(allowed("https://example.com/x", &["example.com"]));
        assert!(allowed("https://a.b.Example.com/x", &["example.com"]));
        assert!(!allowed("https://badexample.com/x", &["example.com"]));
        assert!(!allowed("https://example.com.evil.net/", &["example.com"]));
        assert!(!allowed("ftp://example.com/", &["example.com"]));
        assert!(!allowed("https://example.com/", &[]));
    }

    #[test]
    fn redirect_targets() {
        let base: http::Uri = "https://a.com:8080/x/y".parse().unwrap();
        assert_eq!(join_url(&base, "/z").unwrap(), "https://a.com:8080/z");
        assert_eq!(join_url(&base, "//b.com/z").unwrap(), "https://b.com/z");
        assert_eq!(join_url(&base, "http://c.com/").unwrap(), "http://c.com/");
    }
}
