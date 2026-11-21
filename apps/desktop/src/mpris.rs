//! Media keys and desktop integration through MPRIS, the D-Bus interface
//! Linux desktops use for music players.
//!
//! With it, the play/pause, next and previous keys work, and GNOME, KDE and
//! other desktops show Veena's song, artwork and controls in their media
//! panels and on the lock screen.

use std::cell::RefCell;
use std::rc::Rc;

use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};
use slint::ComponentHandle;
use veena_ffi::{PlayState, PlayerState, RepeatMode, TrackInfo};

use crate::{background, key, with_ui};

thread_local! {
    static PLAYER: RefCell<Option<Rc<Player>>> = const { RefCell::new(None) };
    static LAST: RefCell<Shown> = RefCell::new(Shown::default());
}

/// What the desktop was last told, to send only changes.
#[derive(Default)]
struct Shown {
    track: Option<String>,
    status: Option<PlaybackStatus>,
    shuffle: Option<bool>,
    repeat: Option<RepeatMode>,
    volume: Option<f32>,
    position_ms: u64,
}

/// Register on the session bus. Without one (no desktop session), Veena
/// simply runs without media keys.
pub fn start(window: slint::Weak<crate::AppWindow>) {
    let started = slint::spawn_local(async move {
        let player = match Player::builder("Veena")
            .identity("Veena")
            .desktop_entry("veena-desktop")
            .can_raise(true)
            .can_play(true)
            .can_pause(true)
            .can_go_next(true)
            .can_go_previous(true)
            .can_seek(true)
            .can_control(true)
            .build()
            .await
        {
            Ok(player) => Rc::new(player),
            Err(e) => {
                eprintln!("veena: media keys unavailable: {e}");
                return;
            }
        };

        player.connect_play_pause(|_| command(|v| v.toggle_pause()));
        player.connect_play(|_| command(|v| v.play_current()));
        player.connect_pause(|_| command(|v| v.pause()));
        player.connect_stop(|_| command(|v| v.pause()));
        player.connect_next(|_| command(|v| v.next()));
        player.connect_previous(|_| command(|v| v.previous()));
        player.connect_seek(|_, offset| {
            with_ui(|ui| {
                let at = (ui.state.position_ms as i64 + offset.as_millis()).max(0) as u64;
                background(&ui.veena, move |v| v.seek(at));
            })
        });
        player.connect_set_position(|_, _, position| {
            let at = position.as_millis().max(0) as u64;
            command(move |v| v.seek(at));
        });
        player.connect_set_shuffle(|_, on| command(move |v| v.set_shuffle(on)));
        player.connect_set_loop_status(|_, status| {
            let repeat = match status {
                LoopStatus::None => RepeatMode::Off,
                LoopStatus::Track => RepeatMode::One,
                LoopStatus::Playlist => RepeatMode::All,
            };
            command(move |v| v.set_repeat(repeat));
        });
        player.connect_set_volume(|_, volume| {
            let volume = volume.clamp(0.0, 1.0) as f32;
            with_ui(|ui| {
                if let Some(w) = ui.window.upgrade() {
                    w.set_volume(volume);
                }
                background(&ui.veena, move |v| v.set_volume(volume));
            })
        });
        let raise = window.clone();
        player.connect_raise(move |_| {
            if let Some(w) = raise.upgrade() {
                let _ = w.show();
            }
        });

        // The D-Bus server runs on the window's event loop too.
        let _ = slint::spawn_local(player.run());
        PLAYER.with(|p| *p.borrow_mut() = Some(player));
        with_ui(|ui| update(&ui.state, ui.current().cloned()));
    });
    if let Err(e) = started {
        eprintln!("veena: media keys unavailable: {e}");
    }
}

fn command(f: impl FnOnce(&veena_ffi::Veena) + Send + 'static) {
    with_ui(|ui| background(&ui.veena, f));
}

/// Tell the desktop about the player's state; only what changed is sent.
pub fn update(state: &PlayerState, current: Option<TrackInfo>) {
    let Some(player) = PLAYER.with(|p| p.borrow().clone()) else {
        return;
    };
    let status = match (state.state, &current) {
        (_, None) | (PlayState::Stopped, _) => PlaybackStatus::Stopped,
        (PlayState::Paused, _) => PlaybackStatus::Paused,
        (PlayState::Playing | PlayState::Loading, _) => PlaybackStatus::Playing,
    };
    let track_key = current.as_ref().map(key);

    LAST.with(|last| {
        let mut last = last.borrow_mut();
        let new_track = last.track != track_key;
        let new_status = last.status != Some(status);
        let new_shuffle = last.shuffle != Some(state.shuffle);
        let new_repeat = last.repeat != Some(state.repeat);
        let new_volume = last.volume != Some(state.volume);

        // A jump in position (a seek) is announced; normal progress is not.
        let elapsed_ok = state.position_ms.abs_diff(last.position_ms) < 3000;
        let seeked = !new_track && !elapsed_ok;
        player.set_position(Time::from_millis(state.position_ms as i64));
        last.position_ms = state.position_ms;

        if !(new_track || new_status || new_shuffle || new_repeat || new_volume || seeked) {
            return;
        }
        last.track = track_key.clone();
        last.status = Some(status);
        last.shuffle = Some(state.shuffle);
        last.repeat = Some(state.repeat);
        last.volume = Some(state.volume);

        let metadata = new_track.then(|| metadata(current.as_ref(), state));
        let loop_status = match state.repeat {
            RepeatMode::Off => LoopStatus::None,
            RepeatMode::One => LoopStatus::Track,
            RepeatMode::All => LoopStatus::Playlist,
        };
        let (shuffle, volume, position) =
            (state.shuffle, state.volume as f64, state.position_ms as i64);
        let _ = slint::spawn_local(async move {
            if let Some(metadata) = metadata {
                let _ = player.set_metadata(metadata).await;
            }
            if new_status {
                let _ = player.set_playback_status(status).await;
            }
            if new_shuffle {
                let _ = player.set_shuffle(shuffle).await;
            }
            if new_repeat {
                let _ = player.set_loop_status(loop_status).await;
            }
            if new_volume {
                let _ = player.set_volume(volume).await;
            }
            if seeked {
                let _ = player.seeked(Time::from_millis(position)).await;
            }
        });
    });
}

fn metadata(track: Option<&TrackInfo>, state: &PlayerState) -> Metadata {
    let Some(t) = track else {
        return Metadata::builder().trackid(TrackId::NO_TRACK).build();
    };
    // Track ids are D-Bus object paths: letters, digits and underscores only.
    let id: String = key(t)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut builder = Metadata::builder()
        .title(t.title.clone())
        .artist(t.artist.clone());
    if let Ok(trackid) = TrackId::try_from(format!("/dev/veena/track/{id}")) {
        builder = builder.trackid(trackid);
    }
    if let Some(album) = &t.album {
        builder = builder.album(album.clone());
    }
    if let Some(ms) = state.duration_ms.or(t.duration_ms) {
        builder = builder.length(Time::from_millis(ms as i64));
    }
    if let Some(art) = &t.artwork {
        builder = builder.art_url(art.clone());
    }
    builder.build()
}
