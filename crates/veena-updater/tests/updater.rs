use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use veena_core::Catalog;
use veena_plugin::Options;
use veena_updater::{
    Manifest, ManifestEntry, PluginSettings, State, UpdateError, Updater, generate_signing_key,
    load_installed, parse_signing_key, public_key_of, sha256_hex, sign,
};

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// A web server serving whatever is in `files`, by path.
fn serve(files: Files) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                    break;
                }
            }
            let body = files.lock().unwrap().get(&path).cloned();
            let response = match body {
                Some(body) => {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    r.extend(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = stream.write_all(&response);
        }
    });
    base
}

fn plugin(id: &str, version: &str, title: &str) -> String {
    format!(
        r#"export const meta = {{ id: "{id}", name: "{id}", version: "{version}", api: 1, domains: ["example.com"] }};
export function search() {{ return [{{ id: "x", title: "{title}" + (veena.config.suffix ?? "") }}]; }}"#
    )
}

struct Host {
    files: Files,
    base: String,
    key: ed25519_dalek::SigningKey,
    public: String,
}

impl Host {
    fn new() -> Self {
        let files: Files = Arc::default();
        let base = serve(files.clone());
        let key = parse_signing_key(&generate_signing_key().unwrap()).unwrap();
        let public = public_key_of(&key);
        Self {
            files,
            base,
            key,
            public,
        }
    }

    fn put(&self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.files.lock().unwrap().insert(path.into(), bytes.into());
    }

    /// Publish plugins (id, version, code, settings) under a sequence.
    fn publish(&self, sequence: u64, plugins: &[(&str, &str, &str, PluginSettings)]) {
        let mut entries = Vec::new();
        for (id, version, code, settings) in plugins {
            let path = format!("/files/{id}-{version}.js");
            self.put(&path, code.as_bytes());
            entries.push(ManifestEntry {
                id: id.to_string(),
                version: version.to_string(),
                api: 1,
                url: format!("files/{id}-{version}.js"),
                sha256: sha256_hex(code.as_bytes()),
                settings: settings.clone(),
            });
        }
        let manifest = Manifest {
            format: 1,
            sequence,
            plugins: entries,
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        self.put("/manifest.json.sig", sign(&self.key, &bytes));
        self.put("/manifest.json", bytes);
    }

    fn updater(&self, dir: &std::path::Path) -> Updater {
        Updater::new(dir, &self.public, format!("{}/manifest.json", self.base)).unwrap()
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("veena-updater-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn search_title(dir: &std::path::Path, options: &Options) -> String {
    let plugins = load_installed(dir, options).await.unwrap();
    let (_, plugin) = plugins.into_iter().next().unwrap();
    plugin.unwrap().search("").await.unwrap()[0].title.clone()
}

#[tokio::test]
async fn installs_updates_and_keeps_the_previous_version() {
    let host = Host::new();
    let dir = temp_dir("install");
    let updater = host.updater(&dir);

    host.publish(
        1,
        &[(
            "demo",
            "1.0.0",
            &plugin("demo", "1.0.0", "one"),
            PluginSettings::default(),
        )],
    );
    let report = updater.check().await.unwrap();
    assert_eq!(
        report.installed,
        vec![("demo".into(), None, "1.0.0".into())]
    );
    assert_eq!(search_title(&dir, &Options::default()).await, "one");

    // Same manifest again: nothing to do.
    assert_eq!(updater.check().await.unwrap().unchanged, ["demo"]);

    host.publish(
        2,
        &[(
            "demo",
            "1.1.0",
            &plugin("demo", "1.1.0", "two"),
            PluginSettings::default(),
        )],
    );
    let report = updater.check().await.unwrap();
    assert_eq!(
        report.installed,
        vec![("demo".into(), Some("1.0.0".into()), "1.1.0".into())]
    );
    assert_eq!(search_title(&dir, &Options::default()).await, "two");
    assert!(dir.join(".previous/demo.js").exists());

    // Manual rollback restores 1.0.0 and never reinstalls 1.1.0.
    assert_eq!(updater.rollback("demo").unwrap(), "1.0.0");
    assert_eq!(search_title(&dir, &Options::default()).await, "one");
    let report = updater.check().await.unwrap();
    assert!(
        report
            .skipped
            .iter()
            .any(|(id, why)| id == "demo" && why.contains("failed before")),
        "{report:?}"
    );
    assert!(matches!(
        updater.rollback("demo"),
        Err(UpdateError::NothingToRollBack(_))
    ));
}

#[tokio::test]
async fn refuses_bad_signatures_and_replays() {
    let host = Host::new();
    let dir = temp_dir("signatures");
    let updater = host.updater(&dir);
    host.publish(
        5,
        &[(
            "demo",
            "1.0.0",
            &plugin("demo", "1.0.0", "one"),
            PluginSettings::default(),
        )],
    );
    updater.check().await.unwrap();

    // Tampered manifest: the signature no longer matches.
    let mut bytes = host.files.lock().unwrap()["/manifest.json"].clone();
    bytes[10] ^= 1;
    host.put("/manifest.json", bytes);
    assert!(matches!(
        updater.check().await,
        Err(UpdateError::BadSignature)
    ));

    // Signed by someone else.
    let other = Host::new();
    other.publish(
        9,
        &[(
            "demo",
            "9.0.0",
            &plugin("demo", "9.0.0", "evil"),
            PluginSettings::default(),
        )],
    );
    let wrong_key =
        Updater::new(&dir, &host.public, format!("{}/manifest.json", other.base)).unwrap();
    assert!(matches!(
        wrong_key.check().await,
        Err(UpdateError::BadSignature)
    ));

    // A genuine but older manifest is a replay.
    host.publish(
        4,
        &[(
            "demo",
            "1.0.0",
            &plugin("demo", "1.0.0", "one"),
            PluginSettings::default(),
        )],
    );
    assert!(matches!(
        updater.check().await,
        Err(UpdateError::Replay { found: 4, seen: 5 })
    ));
    assert_eq!(State::load(&dir).unwrap().sequence, 5);
}

#[tokio::test]
async fn rejects_files_that_do_not_match() {
    let host = Host::new();
    let dir = temp_dir("mismatch");
    let updater = host.updater(&dir);
    host.publish(
        1,
        &[
            (
                "hash",
                "1.0.0",
                &plugin("hash", "1.0.0", "ok"),
                PluginSettings::default(),
            ),
            (
                "liar",
                "1.0.0",
                &plugin("other", "1.0.0", "ok"),
                PluginSettings::default(),
            ),
            (
                "broken",
                "1.0.0",
                "export const = ;",
                PluginSettings::default(),
            ),
        ],
    );
    // Replace a file after signing: its hash no longer matches.
    host.put("/files/hash-1.0.0.js", plugin("hash", "1.0.0", "evil"));
    let report = updater.check().await.unwrap();
    let failed: HashMap<_, _> = report.failed.iter().cloned().collect();
    assert!(
        failed["hash"].contains("does not match the signed hash"),
        "{failed:?}"
    );
    assert!(failed["liar"].contains("manifest says liar"), "{failed:?}");
    assert!(failed["broken"].contains("does not load"), "{failed:?}");
    assert!(
        !dir.join("hash.js").exists()
            && !dir.join("liar.js").exists()
            && !dir.join("broken.js").exists()
    );
}

#[tokio::test]
async fn delivers_settings_without_a_new_version() {
    let host = Host::new();
    let dir = temp_dir("settings");
    let updater = host.updater(&dir);
    let code = plugin("demo", "1.0.0", "one");
    host.publish(1, &[("demo", "1.0.0", &code, PluginSettings::default())]);
    updater.check().await.unwrap();

    let mut settings = PluginSettings::default();
    settings.config.insert("suffix".into(), "-tuned".into());
    host.publish(2, &[("demo", "1.0.0", &code, settings)]);
    assert_eq!(updater.check().await.unwrap().settings_changed, ["demo"]);
    assert_eq!(search_title(&dir, &Options::default()).await, "one-tuned");

    // Local settings override delivered ones.
    let mut local = Options::default();
    local.config.insert("suffix".into(), "-local".into());
    assert_eq!(search_title(&dir, &local).await, "one-local");
}

#[tokio::test]
async fn broken_installed_brick_rolls_back_automatically() {
    let host = Host::new();
    let dir = temp_dir("autorollback");
    let updater = host.updater(&dir);
    host.publish(
        1,
        &[(
            "demo",
            "1.0.0",
            &plugin("demo", "1.0.0", "one"),
            PluginSettings::default(),
        )],
    );
    updater.check().await.unwrap();
    host.publish(
        2,
        &[(
            "demo",
            "1.1.0",
            &plugin("demo", "1.1.0", "two"),
            PluginSettings::default(),
        )],
    );
    updater.check().await.unwrap();

    // The installed file gets corrupted on disk.
    std::fs::write(dir.join("demo.js"), "garbage(").unwrap();
    assert_eq!(search_title(&dir, &Options::default()).await, "one");
    let state = State::load(&dir).unwrap();
    assert_eq!(state.plugins["demo"].version, "1.0.0");
    assert!(
        state.plugins["demo"]
            .rejected
            .contains(&"1.1.0".to_string())
    );
}
