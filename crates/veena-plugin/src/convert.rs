//! Conversion between plugin JSON values and Veena types.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use veena_core::{Stream, Track, TrackId};

use crate::PluginError;

/// A track as a plugin returns it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginTrack {
    id: String,
    /// Content kind; defaults to the plugin's first kind.
    kind: Option<String>,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    // JavaScript numbers are floating point, so accept any number.
    duration_ms: Option<f64>,
    artwork: Option<String>,
}

/// A stream as a plugin returns it.
#[derive(Deserialize)]
struct PluginStream {
    url: String,
    mime: Option<String>,
    bitrate: Option<f64>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

/// Either one value or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    Many(Vec<T>),
    One(T),
}

pub fn tracks(default_kind: &str, value: Value) -> Result<Vec<Track>, PluginError> {
    let items: Vec<PluginTrack> = serde_json::from_value(value)
        .map_err(|e| PluginError::InvalidResult(format!("expected a list of tracks: {e}")))?;
    if let Some(bad) = items
        .iter()
        .filter_map(|t| t.kind.as_deref())
        .find(|k| !crate::is_valid_name(k))
    {
        return Err(PluginError::InvalidResult(format!(
            "invalid track kind `{bad}`"
        )));
    }
    Ok(items
        .into_iter()
        .map(|t| Track {
            id: TrackId::new(t.kind.as_deref().unwrap_or(default_kind), t.id),
            title: t.title,
            artist: t.artist.filter(|s| !s.is_empty()),
            album: t.album.filter(|s| !s.is_empty()),
            duration: t
                .duration_ms
                .filter(|ms| ms.is_finite() && *ms >= 0.0)
                .map(|ms| Duration::from_secs_f64(ms / 1000.0)),
            artwork: t.artwork,
        })
        .collect())
}

pub fn streams(value: Value) -> Result<Vec<Stream>, PluginError> {
    let items = match serde_json::from_value::<OneOrMany<PluginStream>>(value)
        .map_err(|e| PluginError::InvalidResult(format!("expected stream objects: {e}")))?
    {
        OneOrMany::Many(items) => items,
        OneOrMany::One(item) => vec![item],
    };
    Ok(items
        .into_iter()
        .map(|s| {
            let mut stream = Stream::http(s.url);
            stream.mime = s.mime;
            stream.bitrate = s
                .bitrate
                .filter(|b| b.is_finite() && *b >= 0.0)
                .map(|b| b.round().min(u32::MAX as f64) as u32);
            s.headers
                .into_iter()
                .fold(stream, |stream, (k, v)| stream.with_header(k, v))
        })
        .collect())
}

/// The track object passed to a plugin's `resolve`.
pub fn track_to_js(track: &Track) -> Value {
    json!({
        "id": track.id.id,
        "kind": track.id.kind,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "durationMs": track.duration.map(|d| d.as_millis() as u64),
        "artwork": track.artwork,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsLyricLine {
    time_ms: Option<f64>,
    text: String,
    #[serde(default)]
    words: Vec<JsLyricWord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsLyricWord {
    time_ms: f64,
    text: String,
}

fn millis(t: f64) -> Option<std::time::Duration> {
    (t.is_finite() && t >= 0.0).then(|| std::time::Duration::from_millis(t as u64))
}

#[derive(Deserialize)]
struct JsLyrics {
    #[serde(default)]
    lines: Vec<JsLyricLine>,
    #[serde(default)]
    instrumental: bool,
}

/// A `lyrics` result: `null` (none found) or `{ lines: [{ timeMs?, text }], instrumental? }`.
pub fn lyrics(value: Value, source: &str) -> Result<Option<veena_core::Lyrics>, PluginError> {
    if value.is_null() {
        return Ok(None);
    }
    let js: JsLyrics =
        serde_json::from_value(value).map_err(|e| PluginError::InvalidResult(e.to_string()))?;
    let lines = js
        .lines
        .into_iter()
        .take(2000)
        .map(|l| veena_core::LyricLine {
            time: l.time_ms.and_then(millis),
            text: l.text.chars().take(500).collect(),
            words: l
                .words
                .into_iter()
                .take(200)
                .filter_map(|w| {
                    Some(veena_core::LyricWord {
                        time: millis(w.time_ms)?,
                        text: w.text.chars().take(100).collect(),
                    })
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    if lines.is_empty() && !js.instrumental {
        return Ok(None);
    }
    Ok(Some(veena_core::Lyrics {
        lines,
        instrumental: js.instrumental,
        source: source.to_string(),
    }))
}
