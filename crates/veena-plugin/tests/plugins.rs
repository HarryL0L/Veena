use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use std::sync::Arc;

use veena_core::{Catalog, Error, Resolver, SourceRegistry, StreamLocation, Track, TrackId};
use veena_plugin::{Limits, Options, PluginBrick, PluginError};

/// A local web server: `/search` returns JSON, `/out` redirects to a
/// disallowed host, `/in` redirects to `/search`.
fn server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut user_agent = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("x-test: ") {
                    user_agent = v.trim().to_owned();
                }
            }
            let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
            let response = if path.starts_with("/search") {
                let body = format!(
                    r#"[{{"id":"a1","name":"Song A","by":"Band","ms":61000}},{{"id":"b2","name":"Song B","by":"","ms":null,"header":"{user_agent}"}}]"#
                );
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else if path == "/out" {
                "HTTP/1.1 302 Found\r\nLocation: http://evil.example/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
            } else if path == "/in" {
                "HTTP/1.1 302 Found\r\nLocation: /search?q=redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
            };
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}")
}

fn meta(extra: &str) -> String {
    format!(
        r#"export const meta = {{ id: "test", name: "Test", version: "1.0.0", api: 1, domains: ["127.0.0.1"] {extra} }};"#
    )
}

/// A complete plugin talking to the test server.
fn plugin_code(base: &str) -> String {
    format!(
        r#"{meta}
const BASE = "{base}";

export async function search(query) {{
  const res = await veena.http.fetch(`${{BASE}}/search?q=${{encodeURIComponent(query)}}`, {{ headers: {{ "X-Test": "hello" }} }});
  if (!res.ok) throw new Error(`status ${{res.status}}`);
  return res.json().map((item) => ({{ id: item.id, title: item.name, artist: item.by, durationMs: item.ms, album: item.header }}));
}}

export function resolve(track) {{
  return {{ url: `${{BASE}}/audio/${{track.id}}.m4a`, mime: "audio/mp4", bitrate: 128000 / 3, headers: {{ Referer: BASE }} }};
}}

export async function playlist(id) {{
  if (id === "redirect-in") {{
    const res = await veena.http.fetch(`${{BASE}}/in`);
    return [{{ id: res.url, title: String(res.status) }}];
  }}
  if (id === "redirect-out") {{ await veena.http.fetch(`${{BASE}}/out`); return []; }}
  if (id === "elsewhere") {{ await veena.http.fetch("https://example.org/"); return []; }}
  if (id === "file") {{ await veena.http.fetch("file:///etc/passwd"); return []; }}
  if (id === "many") {{ for (let i = 0; i < 100; i++) await veena.http.fetch(`${{BASE}}/x`); return []; }}
  if (id === "cache-set") {{ veena.cache.set("k", "v1"); veena.cache.set("short", "x", 0.05); return []; }}
  if (id === "cache-get") {{ return [{{ id: veena.cache.get("k") ?? "none", title: veena.cache.get("short") ?? "expired" }}]; }}
  if (id === "config") {{ return [{{ id: veena.config.client, title: String(veena.apiVersion) }}]; }}
  if (id === "throw") throw new TypeError("boom");
  if (id === "loop") {{ while (true) {{}} }}
  if (id === "attempt") {{
    const loop = veena.attempt(() => {{ while (true) {{}} }});
    const deep = veena.attempt(function f(n) {{ return f(n + 1); }}, 0);
    const fine = veena.attempt((x, y) => x + y, 2, 3);
    const thrown = veena.attempt(() => {{ throw new Error("nope"); }});
    return [loop, deep, fine, thrown].map((r, i) => ({{ id: String(i), title: `${{r.ok}}|${{r.value}}|${{r.error}}` }}));
  }}
  if (id === "recurse") {{ const f = (n) => f(n + 1) + 1; return f(0); }}
  if (id === "deep-json") {{ let v = {{}}; for (let i = 0; i < 150; i++) v = {{ a: [v] }}; const walk = (n) => {{ if (n && typeof n === "object") for (const k in n) walk(n[k]); }}; walk(v); return []; }}
  if (id === "bad-stream") return [{{ id: 1 }}];
  if (id === "fraction") return [{{ id: "f", title: "f", durationMs: 1500.7 }}];
  return [];
}}
"#,
        meta = meta("")
    )
}

fn track(id: &str) -> Track {
    Track::new(TrackId::new("test", id), id)
}

async fn load(code: &str) -> Result<PluginBrick, PluginError> {
    PluginBrick::load(code.to_owned(), Options::default()).await
}

async fn playlist_error(plugin: &PluginBrick, id: &str) -> String {
    match plugin.playlist(id).await {
        Err(e) => e.to_string(),
        Ok(tracks) => panic!("expected an error for {id}, got {tracks:?}"),
    }
}

#[tokio::test]
async fn search_and_resolve() {
    let base = server();
    let plugin = load(&plugin_code(&base)).await.unwrap();
    assert_eq!(plugin.meta().id, "test");
    assert!(plugin.is_catalog() && plugin.is_resolver());
    assert_eq!(plugin.meta().version, "1.0.0");

    let tracks = plugin.search("some song").await.unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].id, TrackId::new("test", "a1"));
    assert_eq!(tracks[0].title, "Song A");
    assert_eq!(tracks[0].artist.as_deref(), Some("Band"));
    assert_eq!(tracks[0].duration, Some(Duration::from_secs(61)));
    assert_eq!(tracks[1].artist, None, "empty strings become None");
    assert_eq!(
        tracks[1].album.as_deref(),
        Some("hello"),
        "request headers are sent"
    );

    let streams = plugin.resolve(&tracks[0]).await.unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].mime.as_deref(), Some("audio/mp4"));
    assert_eq!(streams[0].bitrate, Some(42667));
    match &streams[0].location {
        StreamLocation::Http { url, headers } => {
            assert_eq!(url, &format!("{base}/audio/a1.m4a"));
            assert_eq!(headers, &[("Referer".to_owned(), base.clone())]);
        }
        other => panic!("unexpected location {other:?}"),
    }
}

#[tokio::test]
async fn network_rules() {
    let base = server();
    let plugin = load(&plugin_code(&base)).await.unwrap();

    let redirected = plugin.playlist("redirect-in").await.unwrap();
    assert_eq!(redirected[0].id.id, format!("{base}/search?q=redirected"));
    assert_eq!(redirected[0].title, "200");

    assert!(
        playlist_error(&plugin, "redirect-out")
            .await
            .contains("not allowed to contact evil.example")
    );
    assert!(
        playlist_error(&plugin, "elsewhere")
            .await
            .contains("not allowed to contact example.org")
    );
    let file = playlist_error(&plugin, "file").await;
    assert!(file.contains("only http and https"), "{file}");
    assert!(
        playlist_error(&plugin, "many")
            .await
            .contains("request limit")
    );
}

#[tokio::test]
async fn cache_config_and_errors() {
    let base = server();
    let mut config = serde_json::Map::new();
    config.insert("client".into(), "WEB_REMIX".into());
    let options = Options {
        config,
        ..Options::default()
    };
    let plugin = PluginBrick::load(plugin_code(&base), options)
        .await
        .unwrap();

    plugin.playlist("cache-set").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cached = plugin.playlist("cache-get").await.unwrap();
    assert_eq!(
        (cached[0].id.id.as_str(), cached[0].title.as_str()),
        ("v1", "expired")
    );

    let config = plugin.playlist("config").await.unwrap();
    assert_eq!(
        (config[0].id.id.as_str(), config[0].title.as_str()),
        ("WEB_REMIX", "1")
    );

    assert!(
        playlist_error(&plugin, "throw")
            .await
            .contains("TypeError: boom")
    );
    assert!(
        playlist_error(&plugin, "bad-stream")
            .await
            .contains("invalid data")
    );
    let fraction = plugin.playlist("fraction").await.unwrap();
    assert_eq!(fraction[0].duration.map(|d| d.as_millis()), Some(1500));

    // veena.attempt reports engine limits that try/catch cannot catch.
    let attempts = plugin.playlist("attempt").await.unwrap();
    let results: Vec<&str> = attempts.iter().map(|t| t.title.as_str()).collect();
    assert!(
        results[0].starts_with("false|undefined|") && results[0].contains("loop"),
        "{results:?}"
    );
    assert!(
        results[1].starts_with("false|undefined|") && results[1].contains("recursive"),
        "{results:?}"
    );
    assert_eq!(results[2], "true|5|undefined");
    assert!(
        results[3].starts_with("false|undefined|") && results[3].contains("nope"),
        "{results:?}"
    );

    // Endless recursion is an error, not a crash; deep data is fine.
    let recursed = playlist_error(&plugin, "recurse").await;
    assert!(recursed.contains("recursive calls"), "{recursed}");
    assert!(
        recursed.lines().count() <= 4,
        "trace is trimmed: {recursed}"
    );
    assert!(plugin.playlist("deep-json").await.unwrap().is_empty());

    // An endless loop is stopped by the loop limit; the plugin keeps working.
    let looped = playlist_error(&plugin, "loop").await;
    assert!(looped.to_lowercase().contains("loop"), "{looped}");
    assert_eq!(plugin.search("x").await.unwrap().len(), 2);
}

#[tokio::test]
async fn streams_must_use_allowed_domains() {
    let code = format!(
        r#"{} export async function search() {{ return []; }}
        export async function resolve() {{ return [{{ url: "https://cdn.other.net/a.mp3" }}]; }}"#,
        meta("")
    );
    let plugin = load(&code).await.unwrap();
    let error = plugin.resolve(&track("x")).await.unwrap_err().to_string();
    assert!(
        error.contains("not allowed to contact cdn.other.net"),
        "{error}"
    );
}

#[tokio::test]
async fn trusted_domains_override_declared_ones() {
    let code = format!(
        r#"{} export async function search() {{ return []; }}
        export async function resolve() {{ return [{{ url: "https://cdn.other.net/a.mp3" }}]; }}"#,
        meta("")
    );
    let options = Options {
        domains: Some(vec!["other.net".into()]),
        ..Options::default()
    };
    let plugin = PluginBrick::load(code, options).await.unwrap();
    assert_eq!(plugin.domains(), ["other.net"]);
    assert!(plugin.resolve(&track("x")).await.is_ok());
}

#[tokio::test]
async fn timeout_restarts_plugin() {
    let code = format!(
        r#"{} let calls = 0;
        export function search(q) {{
          calls++;
          if (q === "slow") {{ const end = Date.now() + 3000; while (Date.now() < end) {{}} }}
          return [{{ id: String(calls), title: q }}];
        }}
        export function resolve() {{ return []; }}"#,
        meta("")
    );
    let limits = Limits {
        call_timeout: Duration::from_millis(300),
        loop_iterations: u64::MAX,
        ..Limits::default()
    };
    let plugin = PluginBrick::load(
        code,
        Options {
            limits,
            ..Options::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(plugin.search("a").await.unwrap()[0].id.id, "1");
    let error = plugin.search("slow").await.unwrap_err().to_string();
    assert!(error.contains("took longer"), "{error}");
    // A fresh engine: state from before the restart is gone.
    assert_eq!(plugin.search("b").await.unwrap()[0].id.id, "1");
}

#[tokio::test]
async fn optional_playlist_is_unsupported() {
    let code = format!(
        "{} export function search() {{ return []; }} export function resolve() {{ return []; }}",
        meta("")
    );
    let plugin = load(&code).await.unwrap();
    assert!(matches!(
        plugin.playlist("x").await,
        Err(Error::Unsupported { .. })
    ));
}

#[tokio::test]
async fn load_errors() {
    let functions =
        "export function search() { return []; } export function resolve() { return []; }";

    let old_api = format!(
        r#"export const meta = {{ id: "t", name: "T", version: "1", api: 99 }}; {functions}"#
    );
    assert!(matches!(
        load(&old_api).await,
        Err(PluginError::IncompatibleApi {
            plugin: 99,
            host: 1
        })
    ));

    let reserved = format!(
        r#"export const meta = {{ id: "local", name: "T", version: "1", api: 1 }}; {functions}"#
    );
    assert!(matches!(
        load(&reserved).await,
        Err(PluginError::InvalidMeta(_))
    ));

    let bad_domain = format!(
        r#"export const meta = {{ id: "t", name: "T", version: "1", api: 1, domains: ["*.x.com"] }}; {functions}"#
    );
    assert!(matches!(
        load(&bad_domain).await,
        Err(PluginError::InvalidMeta(_))
    ));

    let no_meta = functions.to_owned();
    assert!(matches!(
        load(&no_meta).await,
        Err(PluginError::InvalidMeta(_))
    ));

    let nothing = meta("");
    assert!(matches!(
        load(&nothing).await,
        Err(PluginError::NothingProvided)
    ));

    let reserved_kind = format!("{} {functions}", meta(r#", kinds: ["file"]"#));
    assert!(matches!(
        load(&reserved_kind).await,
        Err(PluginError::InvalidMeta(_))
    ));

    let import = format!(r#"import fs from "fs"; {} {functions}"#, meta(""));
    assert!(matches!(load(&import).await, Err(PluginError::Load(_))));

    let syntax = "export const = ;";
    assert!(matches!(load(syntax).await, Err(PluginError::Load(_))));

    let top_level_fetch = format!(
        r#"{} await veena.http.fetch("http://127.0.0.1:1/"); {functions}"#,
        meta("")
    );
    match load(&top_level_fetch).await {
        Err(PluginError::Load(message)) => assert!(message.contains("not allowed"), "{message}"),
        other => panic!(
            "expected load error, got {:?}",
            other.map(|p| p.meta().clone())
        ),
    }
}

#[tokio::test]
async fn host_api_is_read_only() {
    let code = format!(
        r#"{}
        export function search() {{
          "use strict";
          const results = [];
          try {{ veena.http.fetch = () => 1; results.push("replaced fetch"); }} catch {{}}
          try {{ veena.http = null; results.push("replaced http"); }} catch {{}}
          try {{ globalThis.veena = null; results.push("replaced veena"); }} catch {{}}
          return results.map((r) => ({{ id: r, title: r }}));
        }}
        export function resolve() {{ return []; }}"#,
        meta("")
    );
    let plugin = load(&code).await.unwrap();
    assert_eq!(plugin.search("").await.unwrap(), vec![]);
}

#[tokio::test]
async fn single_slot_bricks_and_kinds() {
    // A search-only brick labels its tracks with a content kind...
    let catalog = format!(
        r#"{} export function search(q) {{
          return [{{ id: "v1", title: q }}, {{ id: "x", title: "other", kind: "archive" }}];
        }}"#,
        meta(r#", kinds: ["youtube"]"#)
    );
    let catalog = Arc::new(load(&catalog).await.unwrap());
    assert!(catalog.is_catalog() && !catalog.is_resolver());
    let tracks = catalog.search("song").await.unwrap();
    assert_eq!(tracks[0].id, TrackId::new("youtube", "v1"));
    assert_eq!(tracks[1].id, TrackId::new("archive", "x"));

    // ...and a separate audio-link-only brick plays that kind.
    let resolver = r#"export const meta = { id: "yt-audio", name: "YT audio", version: "1", api: 1,
        kinds: ["youtube"], domains: ["127.0.0.1"] };
      export function resolve(track) {
        return [{ url: `http://127.0.0.1/${track.kind}/${track.id}` }];
      }"#;
    let resolver = Arc::new(load(resolver).await.unwrap());
    assert!(!resolver.is_catalog() && resolver.is_resolver());
    assert_eq!(resolver.kinds(), ["youtube"]);

    let registry = SourceRegistry::new();
    catalog.clone().install(&registry);
    resolver.clone().install(&registry);
    assert_eq!(registry.catalogs().len(), 1);
    assert_eq!(registry.resolvers().len(), 1);

    let streams = registry.resolve(&tracks[0]).await.unwrap();
    assert!(
        matches!(&streams[0].location, StreamLocation::Http { url, .. } if url == "http://127.0.0.1/youtube/v1")
    );
    assert!(
        matches!(registry.resolve(&tracks[1]).await, Err(Error::NoResolver(kind)) if kind == "archive")
    );
}

#[tokio::test]
async fn installing_a_new_version_replaces_the_old_one() {
    let both = format!(
        "{} export function search() {{ return []; }} export function resolve() {{ return []; }}",
        meta("")
    );
    let search_only = format!(
        r#"{} export function search() {{ return [{{ id: "new", title: "new" }}]; }}"#,
        meta("")
    );
    let registry = SourceRegistry::new();
    Arc::new(load(&both).await.unwrap()).install(&registry);
    assert_eq!(
        (registry.catalogs().len(), registry.resolvers().len()),
        (1, 1)
    );

    // The update drops `resolve`, so it leaves the audio-link slot.
    Arc::new(load(&search_only).await.unwrap()).install(&registry);
    assert_eq!(
        (registry.catalogs().len(), registry.resolvers().len()),
        (1, 0)
    );
    let tracks = registry.catalog("test").unwrap().search("").await.unwrap();
    assert_eq!(tracks[0].title, "new");
}

struct EchoMinter;

impl veena_plugin::TokenMinter for EchoMinter {
    fn mint(&self, identifier: &str) -> Option<String> {
        Some(format!("token-for-{identifier}"))
    }
}

#[tokio::test]
async fn po_tokens_come_from_the_app() {
    let code = format!(
        "{}\nexport async function search(q) {{ return [{{ id: String(veena.poToken(q)), title: 't' }}]; }}",
        meta("")
    );
    let plugin = load(&code).await.unwrap();
    veena_plugin::set_token_minter(Some(Arc::new(EchoMinter)));
    let with = plugin.search("abc").await.unwrap();
    veena_plugin::set_token_minter(None);
    let without = plugin.search("abc").await.unwrap();
    assert_eq!(with[0].id.id, "token-for-abc");
    assert_eq!(without[0].id.id, "null");
}
