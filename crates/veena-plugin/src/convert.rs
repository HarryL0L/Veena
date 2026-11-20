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

pub fn tracks(source: &str, value: Value) -> Result<Vec<Track>, PluginError> {
    let items: Vec<PluginTrack> = serde_json::from_value(value)
        .map_err(|e| PluginError::InvalidResult(format!("expected a list of tracks: {e}")))?;
    Ok(items
        .into_iter()
        .map(|t| Track {
            id: TrackId::new(source, t.id),
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
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "durationMs": track.duration.map(|d| d.as_millis() as u64),
        "artwork": track.artwork,
    })
}
