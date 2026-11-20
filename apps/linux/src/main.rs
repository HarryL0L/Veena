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

    /// Show log messages, including plugin `console.log` output.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Setting passed to plugins as `veena.config.KEY`. VALUE is JSON, or
    /// plain text. Repeatable, e.g. `--config region=IN`.
    #[arg(long = "config", global = true, value_name = "KEY=VALUE")]
    config: Vec<String>,

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
    /// Get a track's audio link and check that it downloads, without playing.
    /// Useful for testing audio-link bricks.
    Resolve {
        /// Track as `kind:id`, e.g. `youtube:dQw4w9WgXcQ`.
        track: String,
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
    if cli.verbose {
        log::set_logger(&STDERR_LOGGER).ok();
        log::set_max_level(log::LevelFilter::Debug);
    }
    let plugins_dir = cli.plugins.or_else(default_plugins_dir);
    let options = plugin_options(&cli.config)?;
    match cli.command {
        Command::Play { inputs, playback } => {
            let tracks = collect_tracks(&inputs)?;
            let registry = registry(plugins_dir.as_ref(), &options, false).await;
            play(registry, tracks, playback).await
        }
        Command::Find {
            query,
            source,
            limit,
            play: should_play,
            playback,
        } => {
            let registry = registry(plugins_dir.as_ref(), &options, true).await;
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
            let registry = registry(plugins_dir.as_ref(), &options, true).await;
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
            show_plugins(plugins_dir.as_ref(), &options).await;
            Ok(())
        }
        Command::Resolve { track } => {
            let registry = registry(plugins_dir.as_ref(), &options, true).await;
            resolve(&registry, &track).await
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

/// Plugin options from `--config KEY=VALUE` arguments.
fn plugin_options(settings: &[String]) -> Result<Options> {
    let mut config = serde_json::Map::new();
    for setting in settings {
        let Some((key, value)) = setting.split_once('=') else {
            bail!("--config expects KEY=VALUE, got `{setting}`");
        };
        let value = serde_json::from_str(value).unwrap_or_else(|_| value.into());
        config.insert(key.trim().to_owned(), value);
    }
    Ok(Options {
        config,
        ..Options::default()
    })
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
async fn registry(
    plugins_dir: Option<&PathBuf>,
    options: &Options,
    report_failures: bool,
) -> SourceRegistry {
    let registry = SourceRegistry::new();
    // Plays any local path, so no folders need to be scanned up front.
    registry.add_resolver(Arc::new(LocalSource::default()));
    registry.add_resolver(Arc::new(UrlSource));

    let Some(dir) = plugins_dir.filter(|d| d.is_dir()) else {
        return registry;
    };
    match veena_plugin::load_dir(dir, options).await {
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

/// Resolve a track and download the start and middle of its first stream.
async fn resolve(registry: &SourceRegistry, input: &str) -> Result<()> {
    let Some((kind, id)) = input.split_once(':') else {
        bail!("expected `kind:id`, e.g. `youtube:dQw4w9WgXcQ`");
    };
    let names: Vec<String> = registry
        .resolvers_for(kind)
        .iter()
        .map(|r| r.id().to_owned())
        .collect();
    if names.is_empty() {
        bail!("no audio-link brick handles `{kind}` tracks; run `veena plugins`");
    }
    println!("Trying: {}", names.join(" → "));

    let track = Track::new(veena_core::TrackId::new(kind, id), id);
    let started = std::time::Instant::now();
    let streams = registry.resolve(&track).await?;
    println!(
        "Resolved in {:.1}s, {} stream(s):",
        started.elapsed().as_secs_f32(),
        streams.len()
    );
    for stream in &streams {
        let bitrate = stream
            .bitrate
            .map(|b| format!("{} kbps", b / 1000))
            .unwrap_or_default();
        let place = match &stream.location {
            veena_core::StreamLocation::Http { url, .. } => {
                url.split('?').next().unwrap_or(url).to_owned()
            }
            veena_core::StreamLocation::File(path) => path.display().to_string(),
        };
        println!(
            "  {}  {bitrate}  {place}",
            stream.mime.as_deref().unwrap_or("?")
        );
    }

    let veena_core::StreamLocation::Http { url, headers } = streams[0].location.clone() else {
        return Ok(());
    };
    println!("Which request shapes the server accepts:");
    let (check_url, check_headers) = (url.clone(), headers.clone());
    let checks =
        tokio::task::spawn_blocking(move || range_checks(&check_url, &check_headers)).await?;
    checks.iter().for_each(|line| println!("  {line}"));

    println!("Checking the first stream downloads...");
    let report = tokio::task::spawn_blocking(move || probe(&url, &headers)).await?;
    match report {
        Ok(lines) => lines.iter().for_each(|line| println!("  {line}")),
        Err(e) => bail!("download failed: {e}"),
    }
    Ok(())
}

/// Request a stream with different `Range` shapes and report each status.
/// Servers differ in which they accept, and the player depends on it.
fn range_checks(url: &str, headers: &[(String, String)]) -> Vec<String> {
    let agent: veena_net::ureq::Agent = veena_net::config()
        .http_status_as_error(false)
        .build()
        .into();
    // YouTube links state the file size in the `clen` parameter.
    let size: Option<u64> = url
        .split(['?', '&'])
        .find_map(|part| part.strip_prefix("clen="))
        .and_then(|v| v.parse().ok());

    let mut shapes = vec![
        ("first 2 bytes".to_owned(), Some("bytes=0-1".to_owned())),
        ("first 64 KB".to_owned(), Some("bytes=0-65535".to_owned())),
    ];
    // Find the largest range the server accepts.
    for kb in [128u64, 256, 512, 1024] {
        if size.is_none_or(|size| kb * 1024 < size) {
            let label = if kb < 1024 {
                format!("first {kb} KB")
            } else {
                "first 1 MB".into()
            };
            shapes.push((label, Some(format!("bytes=0-{}", kb * 1024 - 1))));
        }
    }
    if let Some(size) = size {
        shapes.push((
            "whole file, exact".to_owned(),
            Some(format!("bytes=0-{}", size - 1)),
        ));
        shapes.push((
            "past the end".to_owned(),
            Some(format!("bytes=0-{}", size + 1_000_000)),
        ));
        shapes.push((
            "from the middle".to_owned(),
            Some(format!("bytes={}-{}", size / 2, size / 2 + 65535)),
        ));
    }
    shapes.push((
        "open-ended (bytes=0-)".to_owned(),
        Some("bytes=0-".to_owned()),
    ));
    shapes.push(("no Range header".to_owned(), None));

    let mut lines = vec![match size {
        Some(size) => format!("file size {size} bytes"),
        None => "file size unknown (no clen in the link)".into(),
    }];
    for (label, range) in shapes {
        let mut request = agent.get(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if let Some(range) = &range {
            request = request.header("Range", range.as_str());
        }
        // Only the status matters; the body is not read.
        let result = match request.call() {
            Ok(response) => format!("{}", response.status().as_u16()),
            Err(e) => format!("error: {e}"),
        };
        lines.push(format!("{label:<24} {result}"));
    }
    lines
}

/// Read the start of a stream, then jump to the middle and read again.
fn probe(url: &str, headers: &[(String, String)]) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};

    let started = std::time::Instant::now();
    let mut reader = veena_audio::HttpReader::open(url, headers)?;
    let mut lines = vec![format!(
        "type {}, size {}, seeking {}",
        reader.content_type().unwrap_or("unknown"),
        reader
            .byte_len()
            .map(|b| format!("{:.1} MB", b as f64 / 1e6))
            .unwrap_or_else(|| "unknown".into()),
        if reader.is_seekable() {
            "supported"
        } else {
            "not supported"
        },
    )];
    let mut buf = vec![0u8; 256 * 1024];
    let mut read = 0;
    while read < buf.len() {
        match reader.read(&mut buf[read..])? {
            0 => break,
            n => read += n,
        }
    }
    let secs = started.elapsed().as_secs_f64();
    lines.push(format!(
        "start: {} KB in {secs:.1}s ({:.0} KB/s)",
        read / 1024,
        read as f64 / 1024.0 / secs
    ));
    if let (true, Some(len)) = (reader.is_seekable(), reader.byte_len()) {
        reader.seek(SeekFrom::Start(len / 2))?;
        let n = reader.read(&mut buf[..16 * 1024])?;
        lines.push(format!("middle: {} KB read after seeking", n / 1024));
    }
    Ok(lines)
}

/// Prints log messages to stderr (enabled with `--verbose`).
struct StderrLogger;

static STDERR_LOGGER: StderrLogger = StderrLogger;

impl log::Log for StderrLogger {
    /// Veena's own messages at any level; other libraries only warnings.
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("veena") || metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

async fn show_plugins(plugins_dir: Option<&PathBuf>, options: &Options) {
    let Some(dir) = plugins_dir else {
        println!("No plugin folder: set --plugins or VEENA_PLUGINS.");
        return;
    };
    let plugins = match veena_plugin::load_dir(dir, options).await {
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
