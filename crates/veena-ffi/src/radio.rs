//! "Up next" recommendations: a catalog suggests songs for the one playing
//! (YouTube Music's radio, for example), and the listener's own library
//! decides which of them to keep and in what order. Nothing about the
//! listener leaves the device.

use std::collections::{HashMap, HashSet};

use veena_core::Track;
use veena_library::{Entry, Played, Skip};

/// Songs added each time the queue is about to run out.
pub const BATCH: usize = 10;
/// Songs played this recently are not suggested again.
const RECENT: usize = 25;
/// At most this many suggestions per artist in one batch, for variety.
const PER_ARTIST: usize = 2;
/// Versions most listeners don't want unless they asked for one.
const ODD_VERSIONS: &[&str] = &[
    "cover",
    "remix",
    "slowed",
    "sped up",
    "speed up",
    "reverb",
    "nightcore",
    "8d",
    "karaoke",
    "instrumental",
    "lofi",
    "lo-fi",
    "bass boosted",
    "1 hour",
    "loop",
];

/// What the listener's library says about their taste.
pub struct Taste<'a> {
    pub history: &'a [Played],
    pub liked: &'a [Entry],
    pub skips: &'a [Skip],
}

/// Pick up to [`BATCH`] of `candidates` to play after `seed`, best first.
/// `queue` is what is already lined up (and is never repeated).
pub fn pick(candidates: Vec<Track>, seed: &Track, queue: &[Track], taste: &Taste) -> Vec<Track> {
    let queued_ids: HashSet<_> = queue.iter().map(|t| &t.id).collect();
    let queued_songs: HashSet<String> = queue
        .iter()
        .chain(std::iter::once(seed))
        .map(song_key)
        .collect();
    let recent: HashSet<_> = taste
        .history
        .iter()
        .take(RECENT)
        .map(|p| &p.entry.track.id)
        .collect();
    let skipped: HashMap<_, u32> = taste.skips.iter().map(|s| (&s.id, s.count)).collect();

    let mut artist_score: HashMap<String, f32> = HashMap::new();
    for entry in taste.liked {
        *artist_score.entry(artist_key(&entry.track)).or_default() += 3.0;
    }
    for played in taste.history {
        *artist_score
            .entry(artist_key(&played.entry.track))
            .or_default() += 0.5;
    }
    for skip in taste.skips {
        let artist = normalize(skip.artist.as_deref().unwrap_or_default());
        *artist_score.entry(first_artist(&artist)).or_default() -= skip.count as f32;
    }

    let seed_title = seed.title.to_lowercase();
    // A place or two lower in a long list shouldn't outweigh taste.
    let total = candidates.len().max(25) as f32;
    let mut scored: Vec<(f32, Track)> = Vec::new();
    for (i, track) in candidates.into_iter().enumerate() {
        if queued_ids.contains(&track.id)
            || recent.contains(&track.id)
            || skipped.get(&track.id).is_some_and(|&n| n >= 2)
        {
            continue;
        }
        // The catalog's own order matters most.
        let mut score = 10.0 * (1.0 - i as f32 / total);
        score += artist_score
            .get(&artist_key(&track))
            .copied()
            .unwrap_or_default()
            .clamp(-6.0, 6.0);
        let title = track.title.to_lowercase();
        if ODD_VERSIONS
            .iter()
            .any(|w| title.contains(w) && !seed_title.contains(w))
        {
            score -= 8.0;
        }
        scored.push((score, track));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut seen_songs = queued_songs;
    let mut picked = Vec::new();
    for (_, track) in scored {
        // The same song again in another upload or version: keep the best.
        if !seen_songs.insert(song_key(&track)) {
            continue;
        }
        let count = per_artist.entry(artist_key(&track)).or_default();
        if *count >= PER_ARTIST {
            continue;
        }
        *count += 1;
        picked.push(track);
        if picked.len() == BATCH {
            break;
        }
    }
    picked
}

/// Lowercase letters and digits only, single spaces.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

/// The main artist, without "Official", "VEVO", "- Topic" and the like.
fn first_artist(artist: &str) -> String {
    let main = artist.split(',').next().unwrap_or_default();
    main.split(' ')
        .filter(|w| !matches!(*w, "official" | "vevo" | "topic" | "music" | "tv"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches("vevo")
        .to_owned()
}

/// The artist, read from "Artist - Title" when the title has it: lyric and
/// fan channels upload many artists' songs under their own name.
fn artist_key(track: &Track) -> String {
    let from_title = track.title.split_once(" - ").map(|(artist, _)| artist);
    let artist = from_title.unwrap_or(track.artist.as_deref().unwrap_or_default());
    first_artist(&normalize(artist))
}

/// Which song this is, whatever the upload: the title without the artist,
/// brackets and extras such as "(Official Video)".
fn song_key(track: &Track) -> String {
    let mut title = track.title.as_str();
    // "Artist - Title"
    if let Some((_, rest)) = title.split_once(" - ") {
        title = rest;
    }
    let mut plain = String::with_capacity(title.len());
    let mut depth = 0;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.split(['|', '•']).next().unwrap_or_default();
    let plain = plain.split(" ft").next().unwrap_or(plain);
    let plain = plain.split(" feat").next().unwrap_or(plain);
    normalize(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veena_core::TrackId;

    fn song(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: TrackId::new("youtube", id),
            title: title.into(),
            artist: Some(artist.into()),
            album: None,
            duration: None,
            artwork: None,
        }
    }

    fn played(track: &Track) -> Played {
        Played {
            entry: Entry {
                track: track.clone(),
                source: String::new(),
            },
            at: 0,
        }
    }

    const NO_TASTE: Taste = Taste {
        history: &[],
        liked: &[],
        skips: &[],
    };

    #[test]
    fn keeps_the_catalog_order_without_any_taste() {
        let seed = song("s", "Bohemian Rhapsody", "Queen");
        let picks = pick(
            vec![
                song("a", "Hotel California", "Eagles"),
                song("b", "Jump", "Van Halen"),
            ],
            &seed,
            std::slice::from_ref(&seed),
            &NO_TASTE,
        );
        assert_eq!(
            picks.iter().map(|t| t.id.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn leaves_out_repeats_versions_and_recent_songs() {
        let seed = song(
            "s",
            "Queen - Bohemian Rhapsody (Official Video)",
            "Queen Official",
        );
        let recent = song("r", "Hotel California", "Eagles");
        let history = [played(&recent)];
        let taste = Taste {
            history: &history,
            liked: &[],
            skips: &[],
        };
        let picks = pick(
            vec![
                song("s", "Bohemian Rhapsody", "Queen"),
                song("s2", "Bohemian Rhapsody (Remastered 2011)", "Queen"),
                recent.clone(),
                song("c", "Jump (cover)", "Someone"),
                song("j", "Jump", "Van Halen"),
            ],
            &seed,
            std::slice::from_ref(&seed),
            &taste,
        );
        let ids: Vec<_> = picks.iter().map(|t| t.id.id.as_str()).collect();
        assert_eq!(ids[0], "j");
        assert!(!ids.contains(&"s") && !ids.contains(&"s2") && !ids.contains(&"r"));
    }

    #[test]
    fn favours_liked_artists_and_drops_songs_skipped_twice() {
        let seed = song("s", "Song", "Someone");
        let liked = [Entry {
            track: song("l", "Old favourite", "Daft Punk"),
            source: String::new(),
        }];
        let skips = [Skip {
            id: TrackId::new("youtube", "x"),
            artist: Some("Nobody".into()),
            count: 2,
            at: 0,
        }];
        let taste = Taste {
            history: &[],
            liked: &liked,
            skips: &skips,
        };
        let picks = pick(
            vec![
                song("x", "Skipped", "Nobody"),
                song("a", "First", "Band"),
                song("d", "Around the World", "Daft Punk"),
            ],
            &seed,
            std::slice::from_ref(&seed),
            &taste,
        );
        let ids: Vec<_> = picks.iter().map(|t| t.id.id.as_str()).collect();
        assert_eq!(ids, ["d", "a"]);
    }

    #[test]
    fn reads_the_artist_from_the_title() {
        let a = song("a", "Survivor - Eye Of The Tiger (Lyrics)", "7clouds Rock");
        let b = song("b", "Eye of the Tiger", "Survivor");
        assert_eq!(artist_key(&a), artist_key(&b));
        assert_eq!(song_key(&a), song_key(&b));
    }

    #[test]
    fn mixes_artists() {
        let seed = song("s", "Song", "Someone");
        let candidates = (0..6)
            .map(|i| song(&format!("q{i}"), &format!("Queen song {i}"), "Queen"))
            .chain([song("e", "Hotel California", "Eagles")])
            .collect();
        let picks = pick(candidates, &seed, std::slice::from_ref(&seed), &NO_TASTE);
        assert_eq!(
            picks
                .iter()
                .filter(|t| t.artist.as_deref() == Some("Queen"))
                .count(),
            2
        );
        assert!(picks.iter().any(|t| t.id.id == "e"));
    }
}
