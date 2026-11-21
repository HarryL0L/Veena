//! Veena for Linux: the desktop app.
//!
//! The window is described in `ui/app.slint`; this file connects it to the
//! same Rust core the Android app uses (search, playback, bricks, library,
//! lyrics and the offline cache).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use slint::{Color, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use veena_ffi::{LyricsInfo, PlayState, PlayerListener, PlayerState, RepeatMode, TrackInfo, Veena};

slint::include_modules!();

/// Bricks shipped with the app; installed on first start like on Android.
const BUNDLED: &[(&str, &str)] = &[
    ("lrclib.js", include_str!("../../../plugins/lrclib.js")),
    (
        "youtube-audio.js",
        include_str!("../../../plugins/youtube-audio.js"),
    ),
    (
        "youtube-music.js",
        include_str!("../../../plugins/youtube-music.js"),
    ),
    (
        "youtube-tv-audio.js",
        include_str!("../../../plugins/youtube-tv-audio.js"),
    ),
    (
        "youtube-videos.js",
        include_str!("../../../plugins/youtube-videos.js"),
    ),
];

/// Name of the video search brick; its results are listed under "Videos".
const VIDEO_SOURCE: &str = "YouTube videos";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    let data = dir("XDG_DATA_HOME", ".local/share");
    let cache = dir("XDG_CACHE_HOME", ".cache");
    install_bundled_bricks(&data.join("plugins"))?;
    let veena = Veena::new(data.display().to_string(), cache.display().to_string())?;

    let window = AppWindow::new()?;
    let ui = Rc::new(RefCell::new(Ui {
        veena: veena.clone(),
        window: window.as_weak(),
        results: Vec::new(),
        recent: veena.history(),
        liked: veena.liked(),
        kept: veena.cached_tracks(),
        state: veena.state(),
        playing_key: None,
        lyrics: None,
    }));
    UI.with(|cell| *cell.borrow_mut() = Some(ui.clone()));
    connect(&window, &ui);
    ui.borrow().refresh_lists();

    // Player updates arrive on a core thread; hand them to the window's.
    veena.clone().set_listener(Box::new(Listener));

    if let Some(shot) = options.screenshot.clone() {
        run_screenshot(&window, shot, options);
    }
    window.run()?;
    Ok(())
}

// ---------------------------------------------------------------- State

thread_local! {
    static UI: RefCell<Option<Rc<RefCell<Ui>>>> = const { RefCell::new(None) };
    /// Screenshots: pause as soon as the song starts, so a machine without
    /// a sound card (which plays instantly) shows the song.
    static PAUSE_ON_START: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn with_ui(f: impl FnOnce(&mut Ui)) {
    UI.with(|cell| {
        if let Some(ui) = cell.borrow().as_ref() {
            f(&mut ui.borrow_mut());
        }
    });
}

/// What the window shows, kept on the window's thread.
struct Ui {
    veena: Arc<Veena>,
    window: slint::Weak<AppWindow>,
    results: Vec<TrackInfo>,
    recent: Vec<TrackInfo>,
    liked: Vec<TrackInfo>,
    kept: Vec<TrackInfo>,
    state: PlayerState,
    playing_key: Option<String>,
    lyrics: Option<(String, LyricsInfo)>,
}

fn key(t: &TrackInfo) -> String {
    format!("{}:{}", t.kind, t.id)
}

impl Ui {
    fn current(&self) -> Option<&TrackInfo> {
        self.state
            .current
            .and_then(|i| self.state.queue.get(i as usize))
    }

    /// The songs in one of the window's lists (see `song-action`).
    fn list(&self, list: i32) -> Vec<TrackInfo> {
        match list {
            0 => self.shown(self.window.upgrade().is_some_and(|w| w.get_videos())),
            1 => self.recent.clone(),
            2 => self.liked.clone(),
            _ => self.kept.clone(),
        }
    }

    /// Search results in the chosen section.
    fn shown(&self, videos: bool) -> Vec<TrackInfo> {
        self.results
            .iter()
            .filter(|t| (t.source == VIDEO_SOURCE) == videos)
            .cloned()
            .collect()
    }

    fn song(&self, t: &TrackInfo, liked: &HashSet<String>, kept: &HashSet<String>) -> Song {
        let k = key(t);
        Song {
            title: t.title.clone().into(),
            artist: t.artist.clone().unwrap_or_default().into(),
            detail: t.source.clone().into(),
            duration: t.duration_ms.map(clock).unwrap_or_default().into(),
            art: art(t.artwork.as_deref()),
            playing: self.playing_key.as_deref() == Some(k.as_str()),
            liked: liked.contains(&k),
            kept: kept.contains(&k),
        }
    }

    /// Rebuild every list of songs (after a search, a like, a new song or new artwork).
    fn refresh_lists(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let liked: HashSet<String> = self.liked.iter().map(key).collect();
        let kept: HashSet<String> = self.kept.iter().map(key).collect();
        let model = |tracks: &[TrackInfo]| -> ModelRc<Song> {
            ModelRc::new(VecModel::from(
                tracks
                    .iter()
                    .map(|t| self.song(t, &liked, &kept))
                    .collect::<Vec<_>>(),
            ))
        };
        window.set_results(model(&self.shown(window.get_videos())));
        window.set_recent(model(
            &self.recent.iter().take(20).cloned().collect::<Vec<_>>(),
        ));
        window.set_liked(model(&self.liked));
        window.set_kept(model(&self.kept));
        if let Some(t) = self.current() {
            window.set_now(self.song(t, &liked, &kept));
        }
        window.set_usage(
            format!(
                "{} kept · {} used",
                songs(self.kept.len()),
                size(self.veena.cache_usage_bytes())
            )
            .into(),
        );
    }

    fn on_state(&mut self, state: PlayerState) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        self.state = state;
        let current = self.current().cloned();
        let new_key = current.as_ref().map(key);
        let changed = new_key != self.playing_key;
        self.playing_key = new_key;

        if self.state.state == PlayState::Playing && PAUSE_ON_START.with(|p| p.replace(false)) {
            background(&self.veena, |v| v.pause());
        }
        window.set_has_song(current.is_some());
        window.set_playing(self.state.state == PlayState::Playing);
        window.set_loading(self.state.state == PlayState::Loading);
        window.set_shuffle(self.state.shuffle);
        window.set_repeat(match self.state.repeat {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        });
        let duration = self
            .state
            .duration_ms
            .or(current.as_ref().and_then(|t| t.duration_ms));
        window.set_position(clock(self.state.position_ms).into());
        window.set_duration(duration.map(clock).unwrap_or_default().into());
        window.set_progress(match duration {
            Some(d) if d > 0 => (self.state.position_ms as f32 / d as f32).clamp(0.0, 1.0),
            _ => 0.0,
        });

        if changed {
            self.lyrics = None;
            window.set_lyrics(ModelRc::default());
            window.set_lyrics_note("Looking for lyrics…".into());
            if let Some(track) = current.clone() {
                // Colours, history, lyrics and the offline list follow the song.
                accent_from(track.artwork.as_deref());
                self.recent = self.veena.history();
                self.kept = self.veena.cached_tracks();
                let veena = self.veena.clone();
                std::thread::spawn(move || {
                    let found = veena.lyrics(track.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        with_ui(|ui| ui.set_lyrics(key(&track), found));
                    });
                });
            }
            self.refresh_lists();
        }
        self.show_lyrics();
    }

    fn set_lyrics(&mut self, for_key: String, found: Option<LyricsInfo>) {
        if self.playing_key.as_deref() != Some(for_key.as_str()) {
            return;
        }
        if let Some(window) = self.window.upgrade() {
            window.set_lyrics_note(match &found {
                Some(l) if l.instrumental => "Instrumental: no words in this one.".into(),
                Some(_) => "".into(),
                None => "No lyrics found for this song.".into(),
            });
        }
        self.lyrics = found.filter(|l| !l.instrumental).map(|l| (for_key, l));
        self.kept = self.veena.cached_tracks();
        self.refresh_lists();
        self.show_lyrics();
    }

    /// The lines around the one being sung, highlighting it.
    fn show_lyrics(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some((_, lyrics)) = &self.lyrics else {
            return;
        };
        let lines: Vec<LyricLine> = if lyrics.synced {
            let now = self.state.position_ms + 300;
            let current = lyrics
                .lines
                .iter()
                .rposition(|l| l.time_ms.is_some_and(|t| t <= now));
            let at = current.map_or(0, |c| c as i64);
            let from = (at - 1).max(0) as usize;
            lyrics
                .lines
                .iter()
                .enumerate()
                .skip(from)
                .take(6)
                .map(|(i, l)| LyricLine {
                    text: if l.text.trim().is_empty() {
                        "♪".into()
                    } else {
                        l.text.clone().into()
                    },
                    state: match current {
                        Some(c) if i == c => 0,
                        Some(c) if i < c => -1,
                        _ => 1,
                    },
                })
                .collect()
        } else {
            lyrics
                .lines
                .iter()
                .take(10)
                .map(|l| LyricLine {
                    text: l.text.clone().into(),
                    state: -1,
                })
                .collect()
        };
        window.set_lyrics(ModelRc::new(VecModel::from(lines)));
    }
}

struct Listener;

impl PlayerListener for Listener {
    fn on_state(&self, state: PlayerState) {
        let _ = slint::invoke_from_event_loop(move || with_ui(|ui| ui.on_state(state)));
    }

    fn on_error(&self, message: String) {
        eprintln!("veena: {message}");
        let _ = slint::invoke_from_event_loop(move || {
            with_ui(|ui| {
                if let Some(window) = ui.window.upgrade() {
                    window.set_search_message(friendly(&message).into());
                }
            })
        });
    }
}

// ---------------------------------------------------------------- Actions

fn connect(window: &AppWindow, ui: &Rc<RefCell<Ui>>) {
    window.set_greeting("Welcome back".into());

    window.on_search(|query| search(query.to_string()));
    window.on_show_videos(|videos| {
        with_ui(|ui| {
            if let Some(w) = ui.window.upgrade() {
                w.set_videos(videos);
            }
            ui.refresh_lists();
        })
    });
    window.on_song_action(|list, index, action| {
        with_ui(|ui| {
            let tracks = ui.list(list);
            match action {
                SongAction::Play => play(ui, tracks, index),
                SongAction::Like => toggle_like(ui, tracks.get(index as usize).cloned()),
                SongAction::PlayNext => {
                    if let Some(t) = tracks.get(index as usize).cloned() {
                        background(&ui.veena, move |v| v.play_next(t));
                    }
                }
                SongAction::Queue => {
                    if let Some(t) = tracks.get(index as usize).cloned() {
                        background(&ui.veena, move |v| v.enqueue(vec![t]));
                    }
                }
            }
        })
    });

    let veena = ui.borrow().veena.clone();
    let v = veena.clone();
    window.on_toggle(move || background(&v, |v| v.toggle_pause()));
    let v = veena.clone();
    window.on_next(move || background(&v, |v| v.next()));
    let v = veena.clone();
    window.on_previous(move || background(&v, |v| v.previous()));
    window.on_seek(|fraction| {
        with_ui(|ui| {
            let total = ui
                .state
                .duration_ms
                .or(ui.current().and_then(|t| t.duration_ms));
            if let Some(total) = total {
                let at = (total as f32 * fraction) as u64;
                background(&ui.veena, move |v| v.seek(at));
            }
        })
    });
    window.on_toggle_shuffle(|| {
        with_ui(|ui| {
            let on = !ui.state.shuffle;
            background(&ui.veena, move |v| v.set_shuffle(on));
        })
    });
    window.on_cycle_repeat(|| {
        with_ui(|ui| {
            let next = match ui.state.repeat {
                RepeatMode::Off => RepeatMode::All,
                RepeatMode::All => RepeatMode::One,
                RepeatMode::One => RepeatMode::Off,
            };
            background(&ui.veena, move |v| v.set_repeat(next));
        })
    });
    window.on_like_now(|| with_ui(|ui| toggle_like(ui, ui.current().cloned())));
    window.on_seek_by(|delta| {
        with_ui(|ui| {
            let at = (ui.state.position_ms as i64 + delta as i64).max(0) as u64;
            background(&ui.veena, move |v| v.seek(at));
        })
    });
    window.on_set_volume(|volume| {
        with_ui(|ui| {
            if let Some(w) = ui.window.upgrade() {
                w.set_volume(volume);
            }
            background(&ui.veena, move |v| v.set_volume(volume));
        })
    });
    let v = veena.clone();
    window.on_set_keep_songs(move |on| v.set_cache_enabled(on));
    let v = veena;
    window.on_set_offline(move |on| v.set_offline(on));
}

/// Run a player command off the window's thread.
fn background(veena: &Arc<Veena>, f: impl FnOnce(&Veena) + Send + 'static) {
    let veena = veena.clone();
    std::thread::spawn(move || f(&veena));
}

fn search(query: String) {
    let query = query.trim().to_string();
    if query.is_empty() {
        return;
    }
    with_ui(|ui| {
        if let Some(w) = ui.window.upgrade() {
            w.set_searching(true);
            w.set_search_message("".into());
            w.set_page(1);
        }
        let veena = ui.veena.clone();
        std::thread::spawn(move || {
            let result = veena.search(query.clone());
            let _ = slint::invoke_from_event_loop(move || {
                with_ui(|ui| {
                    let Some(w) = ui.window.upgrade() else { return };
                    w.set_searching(false);
                    match result {
                        Ok(tracks) => {
                            if tracks.is_empty() {
                                w.set_search_message(
                                    format!("Nothing found for \"{query}\"").into(),
                                );
                            }
                            ui.results = tracks;
                        }
                        Err(e) => w.set_search_message(e.to_string().into()),
                    }
                    ui.refresh_lists();
                })
            });
        });
    });
}

fn play(ui: &Ui, tracks: Vec<TrackInfo>, index: i32) {
    if tracks.is_empty() {
        return;
    }
    let start = (index.max(0) as usize).min(tracks.len() - 1) as u32;
    background(&ui.veena, move |v| v.play(tracks, start));
}

fn toggle_like(ui: &mut Ui, track: Option<TrackInfo>) {
    let Some(track) = track else { return };
    let liked = !ui.veena.is_liked(track.kind.clone(), track.id.clone());
    ui.veena.set_liked(track, liked);
    ui.liked = ui.veena.liked();
    ui.refresh_lists();
}

// ---------------------------------------------------------------- Artwork

type Pixels = SharedPixelBuffer<Rgba8Pixel>;

fn art_cache() -> &'static Mutex<HashMap<String, Option<Pixels>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Pixels>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The picture at `url` if it has arrived; otherwise start fetching it and
/// refresh the lists when it does.
fn art(url: Option<&str>) -> Image {
    let Some(url) = url else {
        return Image::default();
    };
    let mut cache = art_cache().lock().unwrap();
    match cache.get(url) {
        Some(Some(pixels)) => Image::from_rgba8(pixels.clone()),
        Some(None) => Image::default(), // loading or failed
        None => {
            cache.insert(url.to_string(), None);
            let url = url.to_string();
            std::thread::spawn(move || {
                let pixels = fetch_art(&url);
                let ok = pixels.is_some();
                art_cache().lock().unwrap().insert(url, pixels);
                if ok {
                    let _ = slint::invoke_from_event_loop(|| with_ui(|ui| ui.refresh_lists()));
                }
            });
            Image::default()
        }
    }
}

fn fetch_art(url: &str) -> Option<Pixels> {
    fetch_picture(url).or_else(|| {
        // Not every video has the large thumbnail; the standard one always exists.
        url.contains("/hq720.jpg")
            .then(|| url.replace("/hq720.jpg", "/hqdefault.jpg"))
            .and_then(|fallback| fetch_picture(&fallback))
    })
}

fn fetch_picture(url: &str) -> Option<Pixels> {
    let mut response = veena_net::agent().get(url).call().ok()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    let picture = image::load_from_memory(&bytes)
        .ok()?
        .thumbnail(480, 480)
        .to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        picture.as_raw(),
        picture.width(),
        picture.height(),
    ))
}

/// Colour the player from the artwork: a light accent and a deep container,
/// like Material You does from a wallpaper.
fn accent_from(url: Option<&str>) {
    let Some(url) = url.map(str::to_string) else {
        return;
    };
    std::thread::spawn(move || {
        // Wait for the picture if it is still downloading.
        let mut pixels = None;
        for _ in 0..40 {
            if let Some(Some(p)) = art_cache().lock().unwrap().get(&url) {
                pixels = Some(p.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let pixels = pixels.or_else(|| fetch_art(&url))?;
        let (r, g, b) = vivid_average(pixels.as_bytes());
        let _ = slint::invoke_from_event_loop(move || {
            with_ui(|ui| {
                if let Some(w) = ui.window.upgrade() {
                    let theme = w.global::<Theme>();
                    theme.set_accent(mix((r, g, b), (255, 255, 255), 0.55));
                    theme.set_accent_container(mix((r, g, b), (10, 10, 14), 0.62));
                }
            })
        });
        Some(())
    });
}

/// The average colour, weighting colourful pixels far above grey ones.
fn vivid_average(rgba: &[u8]) -> (u8, u8, u8) {
    let (mut r, mut g, mut b, mut weight) = (0f64, 0f64, 0f64, 0f64);
    for px in rgba.chunks_exact(4).step_by(3) {
        let (pr, pg, pb) = (px[0] as f64, px[1] as f64, px[2] as f64);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
        let w = 0.02 + saturation * saturation * (max / 255.0);
        r += pr * w;
        g += pg * w;
        b += pb * w;
        weight += w;
    }
    if weight == 0.0 {
        return (120, 140, 200);
    }
    ((r / weight) as u8, (g / weight) as u8, (b / weight) as u8)
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color::from_rgb_u8(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

// ---------------------------------------------------------------- Helpers

fn dir(env: &str, fallback: &str) -> PathBuf {
    std::env::var_os(env)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
        .join("veena")
}

/// Copy the bundled bricks in, unless the updater manages a newer version.
fn install_bundled_bricks(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let managed = std::fs::read_to_string(dir.join("state.json")).unwrap_or_default();
    for (name, code) in BUNDLED {
        let id = name.trim_end_matches(".js");
        if !managed.contains(&format!("\"{id}\"")) {
            std::fs::write(dir.join(name), code)?;
        }
    }
    Ok(())
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn songs(n: usize) -> String {
    if n == 1 {
        "1 song".into()
    } else {
        format!("{n} songs")
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{} MB", bytes >> 20)
    }
}

fn friendly(message: &str) -> String {
    let title = message
        .strip_prefix("cannot play ")
        .and_then(|rest| rest.split_once(": "))
        .map(|(t, _)| t);
    match title {
        Some(t) => format!("Can't play \"{t}\"."),
        None => "Something went wrong.".into(),
    }
}

// ---------------------------------------------------------------- Screenshots

/// `--screenshot FILE [--search QUERY] [--videos] [--play N] [--page N] [--wait SECONDS]`:
/// drive the app without a person and save a picture of the window. Used to
/// show the app from a machine without a screen.
#[derive(Clone, Default)]
struct Options {
    screenshot: Option<PathBuf>,
    search: Option<String>,
    play: Option<i32>,
    page: Option<i32>,
    videos: bool,
    wait: u64,
}

impl Options {
    fn parse() -> Self {
        let mut options = Options {
            wait: 8,
            ..Default::default()
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
                "--search" => options.search = args.next(),
                "--play" => options.play = args.next().and_then(|n| n.parse().ok()),
                "--page" => options.page = args.next().and_then(|n| n.parse().ok()),
                "--videos" => options.videos = true,
                "--wait" => options.wait = args.next().and_then(|n| n.parse().ok()).unwrap_or(8),
                _ => {}
            }
        }
        options
    }
}

fn run_screenshot(window: &AppWindow, file: PathBuf, options: Options) {
    let weak = window.as_weak();
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        if options.videos {
            with_ui(|ui| {
                if let Some(w) = ui.window.upgrade() {
                    w.set_videos(true);
                }
            });
        }
        if let Some(query) = options.search.clone() {
            search(query);
        }
        let play_at = options.play;
        let page = options.page;
        let weak2 = weak.clone();
        // Give the search time to finish before playing.
        slint::Timer::single_shot(Duration::from_secs(6), move || {
            if let Some(i) = play_at {
                PAUSE_ON_START.with(|p| p.set(true));
                with_ui(|ui| play(ui, ui.list(0), i));
            }
            let weak3 = weak2.clone();
            slint::Timer::single_shot(Duration::from_secs(options.wait), move || {
                if let (Some(w), Some(p)) = (weak3.upgrade(), page) {
                    w.set_page(p);
                }
                let weak4 = weak3.clone();
                let file = file.clone();
                slint::Timer::single_shot(Duration::from_millis(800), move || {
                    if let Some(w) = weak4.upgrade() {
                        match w.window().take_snapshot() {
                            Ok(shot) => {
                                let picture = image::RgbaImage::from_raw(
                                    shot.width(),
                                    shot.height(),
                                    shot.as_bytes().to_vec(),
                                );
                                match picture.map(|p| p.save(&file)) {
                                    Some(Ok(())) => eprintln!("saved {}", file.display()),
                                    other => eprintln!("could not save the picture: {other:?}"),
                                }
                            }
                            Err(e) => eprintln!("could not take a picture: {e}"),
                        }
                    }
                    let _ = slint::quit_event_loop();
                });
            });
        });
    });
}
