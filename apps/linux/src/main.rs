//! Veena for Linux: a terminal music player.
//!
//! This is the first front end for the Veena core. A graphical app will
//! follow; this one exists to exercise playback end to end.

mod terminal;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use veena_core::{Catalog, Repeat, SourceRegistry, Track, UrlSource};
use veena_player::Player;
use veena_plugin::Options;
use veena_src_local::LocalSource;

#[derive(Parser)]
#[command(name = "veena", version, about = "Veena music player")]
struct Cli {
    /// Folder of source plugins (.js files).
    /// Default: ~/.local/share/veena/plugins
    #[arg(long, global = true, env = "VEENA_PLUGINS")]
    plugins: Option<PathBuf>,

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
        #[command(flatten)]
        playback: PlaybackArgs,
    },
    /// Search every installed catalog and optionally play the results.
    Find {
        query: String,
        /// Only search this catalog, e.g. `archive-org`.
        #[arg(short, long)]
        source: Option<String>,
        /// Show at most this many results per source.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        /// Play the results.
        #[arg(short, long)]
        play: bool,
        #[command(flatten)]
        playback: PlaybackArgs,
    },
    /// List a plugin playlist or album, optionally playing it.
    Open {
        /// Catalog id, e.g. `archive-org`.
        source: String,
        /// Playlist or album id.
        id: String,
        /// Play the tracks.
        #[arg(short, long)]
        play: bool,
        #[command(flatten)]
        playback: PlaybackArgs,
    },
    /// Show installed bricks (plugins).
    Plugins,
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

#[derive(clap::Args)]
struct PlaybackArgs {
    /// Play in random order.
    #[arg(long)]
    shuffle: bool,
    /// What to do at the end of a track or the queue.
    #[arg(short, long, value_enum, default_value_t = RepeatArg::Off)]
    repeat: RepeatArg,
    /// Volume from 0 to 100.
    #[arg(short, long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
    volume: u8,
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
    let cli = Cli::parse();
    let plugins_dir = cli.plugins.or_else(default_plugins_dir);
    match cli.command {
        Command::Play { inputs, playback } => {
            let tracks = collect_tracks(&inputs)?;
            let registry = registry(plugins_dir.as_ref(), false).await;
            play(registry, tracks, playback).await
        }
        Command::Find {
            query,
            source,
            limit,
            play: should_play,
            playback,
        } => {
            let registry = registry(plugins_dir.as_ref(), true).await;
            let tracks = find(&registry, &query, source.as_deref(), limit).await?;
            if should_play && !tracks.is_empty() {
                play(registry, tracks, playback).await?;
            }
            Ok(())
        }
        Command::Open {
            source,
            id,
            play: should_play,
            playback,
        } => {
            let registry = registry(plugins_dir.as_ref(), true).await;
            let Some(provider) = registry.catalog(&source) else {
                bail!("no catalog `{source}`; run `veena plugins` to see what is installed");
            };
            let tracks = provider.playlist(&id).await?;
            print_tracks(&tracks);
            if should_play && !tracks.is_empty() {
                play(registry, tracks, playback).await?;
            }
            Ok(())
        }
        Command::Plugins => {
            show_plugins(plugins_dir.as_ref()).await;
            Ok(())
        }
        Command::List { folders } => {
            print_tracks(&LocalSource::new(folders).tracks());
            Ok(())
        }
        Command::Search { query, folders } => {
            print_tracks(&LocalSource::new(folders).search(&query).await?);
            Ok(())
        }
    }
}

/// `$XDG_DATA_HOME/veena/plugins`, falling back to `~/.local/share`.
fn default_plugins_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(data.join("veena/plugins"))
}

/// Built-in bricks plus every plugin that loads.
async fn registry(plugins_dir: Option<&PathBuf>, report_failures: bool) -> SourceRegistry {
    let registry = SourceRegistry::new();
    // Plays any local path, so no folders need to be scanned up front.
    registry.add_resolver(Arc::new(LocalSource::default()));
    registry.add_resolver(Arc::new(UrlSource));

    let Some(dir) = plugins_dir.filter(|d| d.is_dir()) else {
        return registry;
    };
    match veena_plugin::load_dir(dir, &Options::default()).await {
        Ok(plugins) => {
            for (path, plugin) in plugins {
                match plugin {
                    Ok(plugin) => Arc::new(plugin).install(&registry),
                    Err(e) if report_failures => {
                        eprintln!("skipping plugin {}: {e}", path.display());
                    }
                    Err(_) => {}
                }
            }
        }
        Err(e) => eprintln!("cannot read plugins in {}: {e}", dir.display()),
    }
    registry
}

async fn show_plugins(plugins_dir: Option<&PathBuf>) {
    let Some(dir) = plugins_dir else {
        println!("No plugin folder: set --plugins or VEENA_PLUGINS.");
        return;
    };
    let plugins = match veena_plugin::load_dir(dir, &Options::default()).await {
        Ok(plugins) => plugins,
        Err(e) => {
            println!("No plugins in {} ({e}).", dir.display());
            return;
        }
    };
    println!("Plugins in {}\n", dir.display());
    if plugins.is_empty() {
        println!("  none");
    }
    for (path, plugin) in &plugins {
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        match plugin {
            Ok(plugin) => {
                let meta = plugin.meta();
                let slots = match (plugin.is_catalog(), plugin.is_resolver()) {
                    (true, true) => "search + audio links",
                    (true, false) => "search",
                    _ => "audio links",
                };
                println!("  {:<16} {} {}", meta.id, meta.name, meta.version);
                println!(
                    "  {:<16} {slots}  ·  kinds: {}",
                    "",
                    meta.kinds().join(", ")
                );
                println!(
                    "  {:<16} {}  ·  can contact: {}",
                    "",
                    file,
                    plugin.domains().join(", ")
                );
            }
            Err(e) => println!("  {:<16} not loaded: {e}", file),
        }
    }
}

/// Search catalogs and print numbered results. Returns them in order.
async fn find(
    registry: &SourceRegistry,
    query: &str,
    only: Option<&str>,
    limit: usize,
) -> Result<Vec<Track>> {
    let sources: Vec<_> = registry
        .catalogs()
        .into_iter()
        .filter(|s| only.is_none_or(|id| s.id() == id))
        .collect();
    if sources.is_empty() {
        match only {
            Some(id) => {
                bail!("no catalog `{id}`; run `veena plugins` to see what is installed")
            }
            None => bail!(
                "no search bricks installed; add .js plugins to the plugin folder (see `veena plugins`)"
            ),
        }
    }

    let mut found = Vec::new();
    for source in sources {
        match source.search(query).await {
            Ok(tracks) => {
                println!(
                    "{} · {} result{}",
                    source.name(),
                    tracks.len().min(limit),
                    if tracks.len() == 1 { "" } else { "s" }
                );
                for track in tracks.into_iter().take(limit) {
                    found.push(track);
                    let track = found.last().expect("just pushed");
                    let duration = track
                        .duration
                        .map(terminal::format_time)
                        .unwrap_or_default();
                    println!(
                        "{:>4}.  {}  ·  {}  {duration}",
                        found.len(),
                        track.title,
                        track.artist_or_unknown()
                    );
                    println!("       {}", track.id);
                }
            }
            Err(e) => println!("{} · failed: {e}", source.name()),
        }
        println!();
    }
    Ok(found)
}

async fn play(registry: SourceRegistry, tracks: Vec<Track>, args: PlaybackArgs) -> Result<()> {
    if tracks.is_empty() {
        bail!("no playable audio found in the given inputs");
    }
    let player = Player::new(registry).context("cannot start audio playback")?;
    let volume = f32::from(args.volume) / 100.0;
    let repeat = args.repeat.into();
    player.set_volume(volume);
    player.set_repeat(repeat);
    player.enqueue(tracks);
    player.set_shuffle(args.shuffle);
    player.toggle_pause();

    terminal::run(
        &player,
        terminal::Settings {
            volume,
            shuffle: args.shuffle,
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
