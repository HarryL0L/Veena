//! Veena for Linux: a terminal music player.
//!
//! This is the first front end for the Veena core. A graphical app will
//! follow; this one exists to exercise playback end to end.

mod terminal;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use veena_core::{Repeat, Source, SourceRegistry, Track, UrlSource};
use veena_player::Player;
use veena_src_local::LocalSource;

#[derive(Parser)]
#[command(name = "veena", version, about = "Veena music player")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Play files, folders or http(s) links.
    Play {
        /// Audio files, folders (searched recursively) or URLs.
        #[arg(required = true)]
        inputs: Vec<String>,
        /// Play in random order.
        #[arg(short, long)]
        shuffle: bool,
        /// What to do at the end of a track or the queue.
        #[arg(short, long, value_enum, default_value_t = RepeatArg::Off)]
        repeat: RepeatArg,
        /// Volume from 0 to 100.
        #[arg(short, long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
        volume: u8,
    },
    /// List the music found in folders.
    List {
        #[arg(required = true)]
        folders: Vec<PathBuf>,
    },
    /// Search the music in folders by title, artist or album.
    Search {
        query: String,
        /// Folders to search.
        #[arg(short = 'i', long = "in", required = true)]
        folders: Vec<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum RepeatArg {
    Off,
    All,
    One,
}

impl From<RepeatArg> for Repeat {
    fn from(arg: RepeatArg) -> Self {
        match arg {
            RepeatArg::Off => Repeat::Off,
            RepeatArg::All => Repeat::All,
            RepeatArg::One => Repeat::One,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Play {
            inputs,
            shuffle,
            repeat,
            volume,
        } => play(inputs, shuffle, repeat.into(), volume).await,
        Command::List { folders } => {
            let tracks = LocalSource::new(folders).tracks();
            print_tracks(&tracks);
            Ok(())
        }
        Command::Search { query, folders } => {
            let tracks = LocalSource::new(folders).search(&query).await?;
            print_tracks(&tracks);
            Ok(())
        }
    }
}

async fn play(inputs: Vec<String>, shuffle: bool, repeat: Repeat, volume: u8) -> Result<()> {
    let tracks = collect_tracks(&inputs)?;
    if tracks.is_empty() {
        bail!("no playable audio found in the given inputs");
    }

    let mut registry = SourceRegistry::new();
    // Resolves any local path, so no folders need to be scanned up front.
    registry.register(Arc::new(LocalSource::default()));
    registry.register(Arc::new(UrlSource));

    let player = Player::new(registry).context("cannot start audio playback")?;
    let volume = f32::from(volume) / 100.0;
    player.set_volume(volume);
    player.set_repeat(repeat);
    player.enqueue(tracks);
    player.set_shuffle(shuffle);
    player.toggle_pause();

    terminal::run(
        &player,
        terminal::Settings {
            volume,
            shuffle,
            repeat,
        },
    )
    .await
}

/// Turn command-line inputs into tracks, keeping the given order.
fn collect_tracks(inputs: &[String]) -> Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for input in inputs {
        if UrlSource::is_url(input) {
            tracks.push(UrlSource::track(input));
            continue;
        }
        let path = PathBuf::from(input);
        if path.is_dir() {
            tracks.extend(veena_src_local::scan(&[path]));
        } else if path.is_file() {
            tracks.push(LocalSource::track_from_path(&path));
        } else {
            bail!("not found: {input}");
        }
    }
    Ok(tracks)
}

fn print_tracks(tracks: &[Track]) {
    for track in tracks {
        let duration = track
            .duration
            .map(terminal::format_time)
            .unwrap_or_default();
        println!(
            "{:>6}  {}  ·  {}",
            duration,
            track.title,
            track.artist_or_unknown()
        );
    }
    println!(
        "{} track{}",
        tracks.len(),
        if tracks.len() == 1 { "" } else { "s" }
    );
}
