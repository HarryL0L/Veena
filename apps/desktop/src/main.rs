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

mod ambience;
mod mpris;

use ambience::{AmbienceSettings, Moment};

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
    // Media keys and the desktop's media controls.
    mpris::start(window.as_weak());
    // Screenshots stay offline-quiet.
    if options.screenshot.is_none() {
        check_brick_updates(veena.clone());
    }

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

pub(crate) fn with_ui(f: impl FnOnce(&mut Ui)) {
    UI.with(|cell| {
        if let Some(ui) = cell.borrow().as_ref() {
            f(&mut ui.borrow_mut());
        }
    });
}

/// What the window shows, kept on the window's thread.
pub(crate) struct Ui {
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

pub(crate) fn key(t: &TrackInfo) -> String {
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
            4 => self.picks(),
            _ => self.kept.clone(),
        }
    }

    /// Up to six songs from what was played and liked, for Quick picks.
    fn picks(&self) -> Vec<TrackInfo> {
        let mut seen = HashSet::new();
        self.recent
            .iter()
            .chain(self.liked.iter())
            .filter(|t| seen.insert(key(t)))
            .take(6)
            .cloned()
            .collect()
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
        // Quick picks: two rows of three, only once there are a few.
        let picks = self.picks();
        let (a, b) = if picks.len() >= 3 {
            picks.split_at(picks.len().min(3))
        } else {
            (&[][..], &[][..])
        };
        window.set_picks_a(model(a));
        window.set_picks_b(model(b));
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
        mpris::update(&self.state, current);
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
    // The living background: settings from disk, then refreshed every half
    // minute so the sky follows the clock.
    let settings = AmbienceSettings::load();
    window.set_amb_enabled(settings.enabled);
    window.set_amb_day(settings.day_part.unwrap_or(-1));
    window.set_amb_season(settings.season.unwrap_or(-1));
    window.set_amb_southern(settings.southern);
    window.set_amb_special(settings.special_days);
    window.set_autoplay(settings.autoplay);
    ui.borrow().veena.set_autoplay(settings.autoplay);
    show_moment(window);
    let weak = window.as_weak();
    let weak_autoplay = window.as_weak();
    let veena_autoplay = ui.borrow().veena.clone();
    window.on_set_autoplay(move |on| {
        veena_autoplay.set_autoplay(on);
        if let Some(w) = weak_autoplay.upgrade() {
            ambience_settings(&w).save();
        }
    });
    window.on_ambience_changed(move || {
        if let Some(w) = weak.upgrade() {
            ambience_settings(&w).save();
            show_moment(&w);
        }
    });
    let weak = window.as_weak();
    let clock = slint::Timer::default();
    clock.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(30),
        move || {
            if let Some(w) = weak.upgrade() {
                show_moment(&w);
            }
        },
    );
    // The timer lives as long as the app.
    std::mem::forget(clock);

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
                // A search result starts a radio of similar songs.
                SongAction::Play if list == 0 => background(&ui.veena, move |v| {
                    v.play_search_result(tracks, index.max(0) as u32)
                }),
                SongAction::Play => play(ui, tracks, index),
                SongAction::Like => toggle_like(ui, tracks.get(index as usize).cloned()),
                SongAction::PlayNext => {
                    if let Some(t) = tracks.get(index as usize).cloned() {
                        background(&ui.veena, move |v| v.play_next(t));
                    }
                }
                SongAction::RemoveFromHistory => {
                    if let Some(t) = tracks.get(index as usize).cloned() {
                        let k = key(&t);
                        // Gone at once; the core saves the change.
                        ui.recent.retain(|r| key(r) != k);
                        ui.refresh_lists();
                        background(&ui.veena, move |v| v.remove_from_history(t));
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
    window.on_shuffle_liked(|| {
        with_ui(|ui| {
            let liked = ui.liked.clone();
            if liked.is_empty() {
                return;
            }
            // A different song to start with each time.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos() as usize);
            let start = (nanos % liked.len()) as u32;
            background(&ui.veena, move |v| {
                v.set_shuffle(true);
                v.play(liked, start);
            });
        })
    });
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

fn ambience_settings(w: &AppWindow) -> AmbienceSettings {
    let choice = |n: i32| (0..4).contains(&n).then_some(n);
    AmbienceSettings {
        enabled: w.get_amb_enabled(),
        day_part: choice(w.get_amb_day()),
        season: choice(w.get_amb_season()),
        southern: w.get_amb_southern(),
        special_days: w.get_amb_special(),
        autoplay: w.get_autoplay(),
    }
}

/// Greeting, sky and suggestions for right now.
fn show_moment(w: &AppWindow) {
    let m = Moment::now(&ambience_settings(w));
    w.set_scene(m.scene(Color::from_rgb_u8(0x11, 0x13, 0x18)));
    w.set_greeting(m.greeting().into());
    w.set_home_line(m.line().into());
    w.set_date_line(m.date_line().into());
    w.set_moods_title(format!("For this {}", m.moment_name()).into());
    let moods: Vec<slint::SharedString> = m.moods().into_iter().map(Into::into).collect();
    w.set_moods(ModelRc::new(VecModel::from(moods)));
    let mut label = format!(
        "{} · {}",
        ambience::DAY_PARTS[m.day_part()],
        ambience::SEASONS[m.season as usize]
    );
    if m.occasion.is_some() {
        label = format!("{label} · {}", m.greeting());
    }
    w.set_scene_label(label.into());
}

/// Run a player command off the window's thread.
pub(crate) fn background(veena: &Arc<Veena>, f: impl FnOnce(&Veena) + Send + 'static) {
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
            w.set_search_text(query.clone().into());
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
    for px in rgba.as_chunks::<4>().0.iter().step_by(3) {
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
/// Put the app's bricks in the plugin folder; a newer downloaded brick is
/// kept (the core decides which is newer).
fn install_bundled_bricks(dir: &Path) -> Result<(), veena_ffi::VeenaError> {
    let bricks = BUNDLED
        .iter()
        .map(|(name, code)| veena_ffi::BundledBrick {
            name: (*name).to_owned(),
            code: (*code).to_owned(),
        })
        .collect();
    veena_ffi::install_bundled_bricks(dir.display().to_string(), bricks)
}

/// Check for signed brick updates now and then once a day, while the app
/// runs. New bricks are swapped in without restarting.
fn check_brick_updates(veena: Arc<Veena>) {
    let (url, key) = (veena_ffi::brick_update_url(), veena_ffi::brick_update_key());
    if url.is_empty() || key.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("brick-updates".into())
        .spawn(move || {
            loop {
                match veena.update_plugins(url.clone(), key.clone()) {
                    Ok(report) => eprintln!("veena: brick update: {report}"),
                    Err(e) => eprintln!("veena: brick update failed: {e}"),
                }
                std::thread::sleep(Duration::from_secs(24 * 3600));
            }
        });
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

/// `--screenshot FILE [--search QUERY] [--videos] [--play N] [--play-recent N]
/// [--keep-playing] [--seek SECONDS] [--page N] [--size WxH] [--wait SECONDS]
/// [--frames N --interval MS] [--expand-lyrics-at FRAME,FRAME…]`:
/// drive the app without a person and save pictures of the window. Used to
/// show the app from a machine without a screen; several frames make a
/// recording.
#[derive(Clone, Default)]
struct Options {
    screenshot: Option<PathBuf>,
    search: Option<String>,
    play: Option<i32>,
    page: Option<i32>,
    videos: bool,
    wait: u64,
    /// Window size for the picture, e.g. 1360x1500.
    size: Option<(u32, u32)>,
    /// Play this entry of Recently played instead of a search result.
    play_recent: Option<i32>,
    /// Leave the song playing (by default it pauses, for a still picture).
    keep_playing: bool,
    seek: Option<u64>,
    frames: u32,
    interval_ms: u64,
    /// Press the lyrics' expand button when these frames are taken.
    expand_lyrics_at: Vec<u32>,
    /// Move the (paused) song this many milliseconds forward per frame, for
    /// recordings on machines without a real sound device.
    step_ms: Option<u64>,
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
                "--size" => {
                    options.size = args.next().and_then(|s| {
                        let (w, h) = s.split_once('x')?;
                        Some((w.parse().ok()?, h.parse().ok()?))
                    })
                }
                "--play-recent" => options.play_recent = args.next().and_then(|n| n.parse().ok()),
                "--keep-playing" => options.keep_playing = true,
                "--seek" => options.seek = args.next().and_then(|n| n.parse().ok()),
                "--frames" => {
                    options.frames = args.next().and_then(|n| n.parse().ok()).unwrap_or(1)
                }
                "--interval" => {
                    options.interval_ms = args.next().and_then(|n| n.parse().ok()).unwrap_or(250)
                }
                "--step-ms" => options.step_ms = args.next().and_then(|n| n.parse().ok()),
                "--expand-lyrics-at" => {
                    options.expand_lyrics_at = args
                        .next()
                        .map(|list| list.split(',').filter_map(|n| n.parse().ok()).collect())
                        .unwrap_or_default()
                }
                "--wait" => options.wait = args.next().and_then(|n| n.parse().ok()).unwrap_or(8),
                _ => {}
            }
        }
        options
    }
}

fn snapshot(w: &AppWindow, file: &Path) {
    match w.window().take_snapshot() {
        Ok(shot) => {
            let picture =
                image::RgbaImage::from_raw(shot.width(), shot.height(), shot.as_bytes().to_vec());
            if let Some(Err(e)) = picture.map(|p| p.save(file)) {
                eprintln!("could not save the picture: {e}");
            }
        }
        Err(e) => eprintln!("could not take a picture: {e}"),
    }
}

fn run_screenshot(window: &AppWindow, file: PathBuf, options: Options) {
    if let Some((w, h)) = options.size {
        window.window().set_size(slint::PhysicalSize::new(w, h));
    }
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
        let (play_recent, keep_playing, seek) =
            (options.play_recent, options.keep_playing, options.seek);
        slint::Timer::single_shot(Duration::from_secs(6), move || {
            if let Some(i) = play_at {
                PAUSE_ON_START.with(|p| p.set(!keep_playing));
                with_ui(|ui| play(ui, ui.list(0), i));
            }
            if let Some(i) = play_recent {
                PAUSE_ON_START.with(|p| p.set(!keep_playing));
                with_ui(|ui| play(ui, ui.list(1), i));
            }
            if let Some(secs) = seek {
                slint::Timer::single_shot(Duration::from_secs(2), move || {
                    with_ui(|ui| background(&ui.veena, move |v| v.seek(secs * 1000)));
                });
            }
            let weak3 = weak2.clone();
            slint::Timer::single_shot(Duration::from_secs(options.wait), move || {
                if let (Some(w), Some(p)) = (weak3.upgrade(), page) {
                    w.set_page(p);
                }
                let frames = options.frames.max(1);
                let interval = options.interval_ms.max(50);
                let expand_at = options.expand_lyrics_at.clone();
                let (step, start) = (options.step_ms, options.seek.unwrap_or(0) * 1000);
                for n in 0..frames {
                    let weak4 = weak3.clone();
                    let press = expand_at.contains(&n);
                    // Several frames are numbered: shot-000.png, shot-001.png...
                    let file = if frames == 1 {
                        file.clone()
                    } else {
                        file.with_file_name(format!(
                            "{}-{n:03}.png",
                            file.file_stem().unwrap_or_default().to_string_lossy()
                        ))
                    };
                    let last = n + 1 == frames;
                    let delay = Duration::from_millis(800 + interval * n as u64);
                    slint::Timer::single_shot(delay, move || {
                        if let Some(w) = weak4.upgrade() {
                            if press {
                                w.set_lyrics_expanded(!w.get_lyrics_expanded());
                            }
                            if let Some(step) = step {
                                let at = start + step * n as u64;
                                with_ui(|ui| background(&ui.veena, move |v| v.seek(at)));
                            }
                            // A picture right after a press would freeze the window
                            // through the animation; recordings capture it instead.
                            if !press {
                                snapshot(&w, &file);
                            }
                        }
                        if last {
                            let _ = slint::quit_event_loop();
                        }
                    });
                }
            });
        });
    });
}
