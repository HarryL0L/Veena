//! veena-sign: create a signing key and publish signed plugin updates.
//!
//! ```sh
//! veena-sign keygen                          # once; keep the key file secret
//! veena-sign publish --key veena-signing.key --plugins plugins --out site
//! ```
//! Upload the `site` folder anywhere (GitHub Pages, a release, a server) and
//! point Veena at `<url>/manifest.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use veena_plugin::{Options, PluginBrick};
use veena_updater::{
    Manifest, ManifestEntry, PluginSettings, generate_signing_key, parse_signing_key,
    public_key_of, sha256_hex, sign,
};

#[derive(Parser)]
#[command(
    name = "veena-sign",
    version,
    about = "Sign and publish Veena plugin updates"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new signing key and print its public key.
    Keygen {
        /// Where to write the secret key.
        #[arg(long, default_value = "veena-signing.key")]
        out: PathBuf,
    },
    /// Print the public key for a signing key.
    Public {
        #[arg(long, default_value = "veena-signing.key")]
        key: PathBuf,
    },
    /// Build a signed manifest and copy the plugins next to it.
    Publish {
        #[arg(long, default_value = "veena-signing.key")]
        key: PathBuf,
        /// Folder of plugin .js files to publish.
        #[arg(long)]
        plugins: PathBuf,
        /// Output folder to upload.
        #[arg(long)]
        out: PathBuf,
        /// Absolute URL prefix for plugin files; relative by default.
        #[arg(long)]
        base_url: Option<String>,
        /// Manifest sequence; must grow with each publication. Default: now.
        #[arg(long)]
        sequence: Option<u64>,
        /// JSON file of settings per plugin: {"id": {"config": {...}, "domains": [...]}}.
        #[arg(long)]
        settings: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Keygen { out } => keygen(&out),
        Command::Public { key } => {
            println!("{}", public_key_of(&read_key(&key)?));
            Ok(())
        }
        Command::Publish {
            key,
            plugins,
            out,
            base_url,
            sequence,
            settings,
        } => {
            publish(
                &key,
                &plugins,
                &out,
                base_url.as_deref(),
                sequence,
                settings.as_deref(),
            )
            .await
        }
    }
}

fn keygen(out: &Path) -> Result<()> {
    if out.exists() {
        bail!(
            "{} already exists; refusing to overwrite a signing key",
            out.display()
        );
    }
    let secret = generate_signing_key()?;
    std::fs::write(out, format!("{secret}\n"))
        .with_context(|| format!("writing {}", out.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o600))?;
    }
    println!(
        "Secret key written to {} — keep it private and backed up.",
        out.display()
    );
    println!(
        "Public key (build it into Veena):\n{}",
        public_key_of(&parse_signing_key(&secret)?)
    );
    Ok(())
}

fn read_key(path: &Path) -> Result<ed25519_dalek::SigningKey> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse_signing_key(&text)?)
}

async fn publish(
    key: &Path,
    plugins: &Path,
    out: &Path,
    base_url: Option<&str>,
    sequence: Option<u64>,
    settings: Option<&Path>,
) -> Result<()> {
    let key = read_key(key)?;
    let mut settings: BTreeMap<String, PluginSettings> = match settings {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path)?)
            .with_context(|| format!("reading {}", path.display()))?,
        None => BTreeMap::new(),
    };
    std::fs::create_dir_all(out)?;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(plugins)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "js"))
        .collect();
    paths.sort();

    let mut entries = Vec::new();
    for path in paths {
        let code = std::fs::read_to_string(&path)?;
        let brick = PluginBrick::load(code.clone(), Options::default())
            .await
            .with_context(|| format!("{} does not load", path.display()))?;
        let meta = brick.meta();
        let file = format!("{}-{}.js", meta.id, meta.version);
        std::fs::write(out.join(&file), &code)?;
        let url = match base_url {
            Some(base) => format!("{}/{file}", base.trim_end_matches('/')),
            None => file.clone(),
        };
        println!("  {:<20} {:<10} {}", meta.id, meta.version, file);
        entries.push(ManifestEntry {
            id: meta.id.clone(),
            version: meta.version.clone(),
            api: meta.api,
            url,
            sha256: sha256_hex(code.as_bytes()),
            settings: settings.remove(&meta.id).unwrap_or_default(),
        });
    }
    if let Some(unused) = settings.keys().next() {
        bail!(
            "settings given for `{unused}`, which is not in {}",
            plugins.display()
        );
    }

    let sequence = sequence.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    });
    let manifest = Manifest {
        format: veena_updater::MANIFEST_FORMAT,
        sequence,
        plugins: entries,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(out.join("manifest.json"), &bytes)?;
    std::fs::write(out.join("manifest.json.sig"), sign(&key, &bytes))?;
    println!(
        "Signed manifest (sequence {sequence}) written to {}",
        out.display()
    );
    Ok(())
}
