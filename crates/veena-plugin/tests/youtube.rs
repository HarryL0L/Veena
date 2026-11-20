//! The YouTube bricks in `plugins/`, run against a fake YouTube server.
//!
//! The responses mimic the shape of YouTube's API. They check the bricks'
//! logic (parsing, fallback, format choice), not that YouTube still
//! answers this way; that needs a live test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use veena_core::{Catalog, Resolver, StreamLocation, Track, TrackId};
use veena_plugin::{Options, PluginBrick};

#[derive(Default)]
struct Seen {
    search_bodies: Vec<Value>,
    visitor_headers: Vec<String>,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> &str {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }
}

fn serve(seen: Arc<Mutex<Seen>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stream_base = base.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    headers.push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
            let mut request = Request {
                method,
                path,
                headers,
                body: String::new(),
            };
            let length: usize = request.header("content-length").parse().unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.body = String::from_utf8(body).unwrap();

            let (content_type, body) = respond(&request, &seen, &stream_base);
            // Like YouTube refusing links that need a token.
            let range_start: u64 = request
                .header("range")
                .trim_start_matches("bytes=")
                .split('-')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            // `/firstmb` links only serve the start of the file.
            let tv_refused = request.path.starts_with("/tvstream") && !tv_link_valid(&request.path);
            let status = if request.path.starts_with("/forbidden")
                || tv_refused
                || (request.path.starts_with("/firstmb") && range_start > 1_000_000)
            {
                "403 Forbidden"
            } else {
                "200 OK"
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    base
}

fn respond(request: &Request, seen: &Mutex<Seen>, base: &str) -> (&'static str, String) {
    let path = request.path.split('?').next().unwrap_or_default();
    match (request.method.as_str(), path) {
        ("GET", "/") => (
            "text/html",
            r#"<html><script>ytcfg.set({"INNERTUBE_CLIENT_VERSION":"1.20990101.00.00","VISITOR_DATA":"VIS123"});</script></html>"#.into(),
        ),
        ("POST", "/youtubei/v1/search") => {
            let body: Value = serde_json::from_str(&request.body).unwrap();
            let mut seen = seen.lock().unwrap();
            seen.visitor_headers.push(request.header("x-goog-visitor-id").to_owned());
            let query = body["query"].as_str().unwrap_or_default().to_owned();
            seen.search_bodies.push(body);
            if query == "nothing" {
                return ("application/json", json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [{"messageRenderer": {"text": {"runs": [{"text": "No results"}]}}}]}}]}}}}]}}}).to_string());
            }
            if query == "changed" {
                return ("application/json", json!({"responseContext": {}, "somethingNew": {}}).to_string());
            }
            ("application/json", search_response().to_string())
        }
        ("POST", "/youtubei/v1/browse") => ("application/json", json!({
            "contents": {"twoColumnBrowseResultsRenderer": {"secondaryContents": {"sectionListRenderer": {"contents": [
                {"musicShelfRenderer": {"contents": [
                    row("a1", "Album Song One", &[artist_run("Band")], Some("3:01")),
                    row("a2", "Album Song Two", &[artist_run("Band")], Some("4:15")),
                ]}}
            ]}}}}
        }).to_string()),
        ("GET", "/iframe_api") => (
            "text/javascript",
            r"var scriptUrl = 'https:\/\/www.youtube.com\/s\/player\/0123abcd\/www-widgetapi.vflset\/www-widgetapi.js';".into(),
        ),
        ("GET", "/s/player/0123abcd/player_ias.vflset/en_US/base.js") => {
            ("text/javascript", FAKE_PLAYER.into())
        }
        ("POST", "/youtubei/v1/player") if request.header("user-agent").contains("Cobalt") => {
            let body: Value = serde_json::from_str(&request.body).unwrap();
            let sts = &body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"];
            let response = if sts != 20123 {
                json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": format!("bad signature timestamp {sts}")}})
            } else {
                let stream = format!("{base}/tvstream?id=hi&n={SCRAMBLED_N}");
                json!({
                    "playabilityStatus": {"status": "OK"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "contentLength": "5000000",
                         "signatureCipher": format!("s={}&sp=sig&url={}", SCRAMBLED_SIGNATURE, percent_encode(&stream))},
                        {"itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 50000, "contentLength": "2000000",
                         "url": format!("{base}/tvstream?id=lo&n={SCRAMBLED_N}")},
                        {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "url": format!("{base}/opus")},
                    ]}
                })
            };
            ("application/json", response.to_string())
        }
        ("POST", "/youtubei/v1/player") => {
            let body: Value = serde_json::from_str(&request.body).unwrap();
            let video = body["videoId"].as_str().unwrap_or_default();
            let user_agent = request.header("user-agent");
            let has_visitor = request.header("x-goog-visitor-id") == "VIS123"
                && body["context"]["client"]["visitorData"] == "VIS123";
            // Like YouTube: one app type needs a visitor id, another is
            // switched off for this video.
            let response = if (user_agent.contains("oculus") && !has_visitor) || (user_agent.contains("ios") && video == "ios-off") {
                json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm you're not a bot"}})
            } else if video == "denied" {
                json!({
                    "playabilityStatus": {"status": "OK"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "url": format!("{base}/forbidden")},
                    ]}
                })
            } else if video == "firstmb" {
                json!({
                    "playabilityStatus": {"status": "OK"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "contentLength": "5000000", "url": format!("{base}/firstmb")},
                    ]}
                })
            } else if video == "opus-only" {
                json!({
                    "playabilityStatus": {"status": "OK"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "url": format!("{base}/opus")},
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "signatureCipher": "s=x"},
                    ]}
                })
            } else if video == "blocked" {
                json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"}})
            } else {
                json!({
                    "playabilityStatus": {"status": "OK"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "url": format!("{base}/opus")},
                        {"itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 50000, "url": format!("{base}/low")},
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130000, "averageBitrate": 129500, "url": format!("{base}/high?id={video}")},
                        {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 260000, "signatureCipher": "s=abc&url=x"},
                        {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1\"", "bitrate": 4000000, "url": format!("{base}/video")},
                    ]}
                })
            };
            ("application/json", response.to_string())
        }
        _ => ("text/plain", String::new()),
    }
}

fn artist_run(name: &str) -> Value {
    json!({"text": name, "navigationEndpoint": {"browseEndpoint": {"browseId": "UC1", "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_ARTIST"}}}}})
}

fn album_run(name: &str) -> Value {
    json!({"text": name, "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_1", "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_ALBUM"}}}}})
}

/// A song row like YouTube Music's `musicResponsiveListItemRenderer`.
fn row(video: &str, title: &str, details: &[Value], duration: Option<&str>) -> Value {
    let mut runs: Vec<Value> = Vec::new();
    for (i, run) in details.iter().enumerate() {
        if i > 0 {
            runs.push(json!({"text": " • "}));
        }
        runs.push(run.clone());
    }
    if let Some(duration) = duration {
        runs.push(json!({"text": " • "}));
        runs.push(json!({"text": duration}));
    }
    json!({"musicResponsiveListItemRenderer": {
        "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
            {"url": "https://i.ytimg.com/small.jpg", "width": 60},
            {"url": "https://i.ytimg.com/large.jpg", "width": 120}
        ]}}},
        "flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title, "navigationEndpoint": {"watchEndpoint": {"videoId": video}}}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}}
        ],
        "playlistItemData": {"videoId": video}
    }})
}

fn search_response() -> Value {
    json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicShelfRenderer": {"contents": [
            row("vid1", "Doodle", &[artist_run("Zachz Winner"), album_run("Doodle - Single")], Some("2:45")),
            row("vid2", "Duet", &[artist_run("Singer A"), json!({"text": " & "}), artist_run("Singer B")], Some("1:02:03")),
            row("vid1", "Doodle", &[artist_run("Zachz Winner")], Some("2:45")),
            json!({"musicResponsiveListItemRenderer": {"flexColumns": []}}),
        ]}}
    ]}}}}]}}})
}

/// A cut-down imitation of YouTube's base.js: the signature function uses
/// helper steps in an object, a decoy looks similar but changes nothing,
/// the n function is long, and setup code fails without a real page.
const FAKE_PLAYER: &str = r#"var _yt_player={};(function(g){var window=this;
var T=["sp"+"lit","","jo"+"in","decodeURI"+"Component"];
var Zq={Ab:function(a,b){a.splice(0,b)},Cd:function(a){a.reverse()},Ef:function(a,b){var c=a[0];a[0]=a[b%a.length];a[b%a.length]=c}};
var Decoy=function(a){a=a[T[0]](T[1]);return a[T[2]](T[1])};
var Sig=function(x,a){a=a[T[0]](T[1]);Zq.Cd(a,7);Zq.Ab(a,x);Zq.Ef(a,40);return a[T[2]](T[1])};
var Nfn=function(a){var b=a[T[0]](T[1]),c=[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180];
try{for(var i=0;i<b.length;i++){b[i]=String.fromCharCode((b[i].charCodeAt(0)-48+7)%75+48)}b.reverse()}catch(d){return"enhanced_except_"+a}return b[T[2]](T[1])};
var Yx=[Nfn];
var Loopy=function(a){var b=a;while(true){}return b};
var use=function(a,b){var c=a.get("n");c&&(c=Loopy(c));c&&(c=Yx[0](c),a.set("n",c));var s=b.s;s&&(b.sig=Sig(3,window[T[3]](s)))};
var cfg={signatureTimestamp:20123,flags:{}};
new Intl.NumberFormat("en").format(1);
document.createElement("video").canPlayType("audio/mp4");
navigator.mediaDevices.enumerateDevices().then(function(){});
throw new Error("needs a real page");
})(_yt_player);
"#;

const SCRAMBLED_SIGNATURE: &str = "AOq0QJ8wRAIgXmPlOPSBkkUs1bYFYlJCfe29xxLTnR1o6ZK7Vb1m1q4CIGR5Iu0ADBc0bMgDGNIq6GvdhZkTHvXlrrjZOq9Ki7Vfxyz";
const SCRAMBLED_N: &str = "q8Lm3Nz5Rt7Wb9Yc";

/// What the fake player's signature function computes.
fn expected_signature(s: &str) -> String {
    let mut a: Vec<char> = s.chars().collect();
    a.reverse();
    a.drain(..3);
    let len = a.len();
    a.swap(0, 40 % len);
    a.into_iter().collect()
}

/// What the fake player's n function computes.
fn expected_n(n: &str) -> String {
    let mut out: Vec<char> = n
        .chars()
        .map(|c| char::from_u32((c as u32 - 48 + 7) % 75 + 48).unwrap())
        .collect();
    out.reverse();
    out.into_iter().collect()
}

/// A `/tvstream` link is valid only with the right n, and the right
/// signature when one is present.
fn tv_link_valid(path: &str) -> bool {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(percent_decode)
    };
    let n_ok = param("n").as_deref() == Some(expected_n(SCRAMBLED_N).as_str());
    let sig_ok = match param("id").as_deref() {
        Some("hi") => {
            param("sig").as_deref() == Some(expected_signature(SCRAMBLED_SIGNATURE).as_str())
        }
        _ => true,
    };
    n_ok && sig_ok
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// The app types youtube-audio.js tries, in order.
const CLIENTS: [&str; 4] = [
    "ANDROID_VR 1.65.10",
    "ANDROID_VR 1.62.27",
    "ANDROID_VR 1.60.19",
    "IOS 20.10.4",
];

fn plugin_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name)
}

async fn load(name: &str, base: &str) -> PluginBrick {
    let mut config = serde_json::Map::new();
    config.insert("baseUrl".into(), base.into());
    let options = Options {
        config,
        domains: Some(vec!["127.0.0.1".into()]),
        ..Options::default()
    };
    PluginBrick::load_file(&plugin_path(name), options)
        .await
        .unwrap()
}

#[tokio::test]
async fn search_reads_songs() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone());
    let music = load("youtube-music.js", &base).await;
    assert!(music.is_catalog() && !music.is_resolver());

    let tracks = music.search("doodle").await.unwrap();
    assert_eq!(
        tracks.len(),
        2,
        "duplicates and broken rows are skipped: {tracks:#?}"
    );
    let doodle = &tracks[0];
    assert_eq!(doodle.id, TrackId::new("youtube", "vid1"));
    assert_eq!(doodle.title, "Doodle");
    assert_eq!(doodle.artist.as_deref(), Some("Zachz Winner"));
    assert_eq!(doodle.album.as_deref(), Some("Doodle - Single"));
    assert_eq!(doodle.duration, Some(Duration::from_secs(165)));
    assert_eq!(
        doodle.artwork.as_deref(),
        Some("https://i.ytimg.com/large.jpg")
    );
    assert_eq!(tracks[1].artist.as_deref(), Some("Singer A, Singer B"));
    assert_eq!(tracks[1].duration, Some(Duration::from_secs(3723)));

    // The client version and visitor id come from the home page.
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.search_bodies[0]["context"]["client"]["clientVersion"],
        "1.20990101.00.00"
    );
    assert_eq!(
        seen.search_bodies[0]["context"]["client"]["clientName"],
        "WEB_REMIX"
    );
    assert_eq!(seen.visitor_headers[0], "VIS123");
}

#[tokio::test]
async fn search_edge_cases() {
    let base = serve(Arc::default());
    let music = load("youtube-music.js", &base).await;
    assert_eq!(music.search("   ").await.unwrap(), vec![]);
    assert_eq!(music.search("nothing").await.unwrap(), vec![]);
    let error = music.search("changed").await.unwrap_err().to_string();
    assert!(error.contains("could not read search results"), "{error}");

    let album = music.playlist("MPREb_1").await.unwrap();
    let titles: Vec<&str> = album.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Album Song One", "Album Song Two"]);
}

#[tokio::test]
async fn audio_links_fall_back_and_pick_aac() {
    let base = serve(Arc::default());
    let audio = load("youtube-audio.js", &base).await;
    assert!(audio.is_resolver() && !audio.is_catalog());

    // The visitor id from the home page gets the first app type through.
    let track = Track::new(TrackId::new("youtube", "vid1"), "Doodle");
    let streams = audio.resolve(&track).await.unwrap();
    assert_eq!(streams.len(), 2, "only AAC with direct links: {streams:#?}");
    assert_eq!(streams[0].bitrate, Some(129_500));
    assert_eq!(streams[1].bitrate, Some(50_000));
    assert_eq!(streams[0].mime.as_deref(), Some("audio/mp4"));
    match &streams[0].location {
        StreamLocation::Http { url, headers } => {
            assert_eq!(url, &format!("{base}/high?id=vid1"));
            assert!(
                headers
                    .iter()
                    .any(|(k, v)| k == "User-Agent" && v.contains("oculus"))
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    // Links that are refused on download are skipped, not returned.
    let denied = Track::new(TrackId::new("youtube", "denied"), "x");
    let error = audio.resolve(&denied).await.unwrap_err().to_string();
    for client in CLIENTS {
        let expected =
            format!("{client}: links refused when downloading (status 403 at the start)");
        assert!(error.contains(&expected), "{error}");
    }

    // Links that only serve the first megabyte are caught too.
    let first_mb = Track::new(TrackId::new("youtube", "firstmb"), "x");
    let error = audio.resolve(&first_mb).await.unwrap_err().to_string();
    assert!(
        error.contains("IOS 20.10.4: links refused when downloading (status 403 past the first megabyte; YouTube wants a proof-of-origin token)"),
        "{error}"
    );

    let opus = Track::new(TrackId::new("youtube", "opus-only"), "x");
    let error = audio.resolve(&opus).await.unwrap_err().to_string();
    assert!(
        error.contains("1 audio/webm with links, 1 audio/mp4 needing signatures"),
        "{error}"
    );

    let blocked = Track::new(TrackId::new("youtube", "blocked"), "x");
    let error = audio.resolve(&blocked).await.unwrap_err().to_string();
    for client in CLIENTS {
        let expected = format!("{client}: UNPLAYABLE: Video unavailable");
        assert!(error.contains(&expected), "{error}");
    }
}

#[tokio::test]
async fn falls_back_when_the_first_app_type_is_refused() {
    let base = serve(Arc::default());
    let mut config = serde_json::Map::new();
    config.insert("baseUrl".into(), base.clone().into());
    // Without the home page's visitor id, the first app type is refused.
    let options = Options {
        config,
        domains: Some(vec!["127.0.0.1".into()]),
        ..Options::default()
    };
    let code = std::fs::read_to_string(plugin_path("youtube-audio.js"))
        .unwrap()
        .replace("/\"VISITOR_DATA\":\"([^\"]+)\"/", "/NO_VISITOR_DATA_HERE/");
    let audio = PluginBrick::load(code, options).await.unwrap();
    let track = Track::new(TrackId::new("youtube", "vid1"), "x");
    let streams = audio.resolve(&track).await.unwrap();
    match &streams[0].location {
        StreamLocation::Http { headers, .. } => {
            assert!(
                headers
                    .iter()
                    .any(|(k, v)| k == "User-Agent" && v.contains("ios.youtube"))
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn tv_brick_solves_the_player_puzzles() {
    let base = serve(Arc::default());
    let tv = load("youtube-tv-audio.js", &base).await;
    assert!(tv.is_resolver() && !tv.is_catalog());

    let track = Track::new(TrackId::new("youtube", "vid1"), "Doodle");
    let streams = tv.resolve(&track).await.unwrap();
    assert_eq!(streams.len(), 2, "AAC only: {streams:#?}");
    let StreamLocation::Http { url, headers } = &streams[0].location else {
        panic!("unexpected {:?}", streams[0].location);
    };
    let path = url.strip_prefix(&base).unwrap();
    assert!(tv_link_valid(path), "signature and n unscrambled: {url}");
    assert!(
        headers
            .iter()
            .any(|(k, v)| k == "User-Agent" && v.contains("Cobalt"))
    );
    let StreamLocation::Http { url, .. } = &streams[1].location else {
        panic!()
    };
    assert!(
        tv_link_valid(url.strip_prefix(&base).unwrap()),
        "n unscrambled: {url}"
    );

    // The solved player is reused for the next track.
    assert_eq!(tv.resolve(&track).await.unwrap().len(), 2);
}
