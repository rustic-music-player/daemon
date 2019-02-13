extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate failure;
extern crate toml;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate log;
extern crate ctrlc;
extern crate env_logger;

// Core
extern crate rustic_core as rustic;

// Frontends
extern crate rustic_http_frontend as http_frontend;
extern crate rustic_mpd_frontend as mpd_frontend;

// Stores
extern crate rustic_memory_store as memory_store;
extern crate rustic_sqlite_store as sqlite_store;

// Backends
extern crate rustic_gstreamer_backend as gst_backend;

// Provider
extern crate rustic_local_provider as local_provider;
extern crate rustic_pocketcasts_provider as pocketcasts_provider;
extern crate rustic_soundcloud_provider as soundcloud_provider;
extern crate rustic_spotify_provider as spotify_provider;

use failure::Error;
use memory_store::MemoryLibrary;
use sqlite_store::SqliteLibrary;
use std::fs::File;
use std::io::prelude::*;
use std::sync::{Arc, Condvar, Mutex, RwLock};

#[derive(Deserialize, Clone, Default)]
pub struct Config {
    mpd: Option<mpd_frontend::MpdConfig>,
    http: Option<http_frontend::HttpConfig>,
    pocketcasts: Option<pocketcasts_provider::PocketcastsProvider>,
    soundcloud: Option<soundcloud_provider::SoundcloudProvider>,
    spotify: Option<spotify_provider::SpotifyProvider>,
    local: Option<local_provider::LocalProvider>,
    library: Option<LibraryConfig>,
    #[serde(default)]
    backend: BackendConfig,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "store", rename_all = "lowercase")]
pub enum LibraryConfig {
    Memory,
    SQLite { path: String },
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum BackendConfig {
    GStreamer,
    Rodio,
}

impl Default for BackendConfig {
    fn default() -> BackendConfig {
        BackendConfig::GStreamer
    }
}

fn read_config() -> Config {
    let mut config_file = File::open("config.toml").unwrap();
    let mut config = String::new();
    config_file.read_to_string(&mut config).unwrap();
    toml::from_str(config.as_str()).unwrap()
}

fn main() -> Result<(), Error> {
    env_logger::init();

    let config = read_config();
    let mut providers: rustic::provider::SharedProviders = vec![];

    if config.pocketcasts.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.pocketcasts.unwrap()))));
    }
    if config.soundcloud.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.soundcloud.unwrap()))));
    }
    if config.spotify.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.spotify.unwrap()))));
    }
    if config.local.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.local.unwrap()))));
    }

    for provider in &providers {
        let mut provider = provider.write().unwrap();
        provider.setup().unwrap_or_else(|err| {
            error!("Can't setup {} provider: {:?}", provider.title(), err)
        });
    }

    let store: Box<rustic::Library> = match config.library.unwrap_or(LibraryConfig::Memory) {
        LibraryConfig::Memory => Box::new(MemoryLibrary::new()),
        LibraryConfig::SQLite { path } => Box::new(SqliteLibrary::new(path)?),
    };

    let backend = match config.backend {
        BackendConfig::GStreamer => gst_backend::GstBackend::new()?,
        _ => panic!("invalid backend config"),
    };

    let app = rustic::Rustic::new(store, providers, backend)?;

    let keep_running = Arc::new((Mutex::new(true), Condvar::new()));

    let interrupt = Arc::clone(&keep_running);

    ctrlc::set_handler(move || {
        info!("Shutting down");
        let &(ref lock, ref cvar) = &*interrupt;
        let mut running = lock.lock().unwrap();
        *running = false;
        cvar.notify_all();
    })?;

    let mut threads = vec![
        rustic::sync::start(Arc::clone(&app), Arc::clone(&keep_running))?,
        rustic::cache::start(Arc::clone(&app), Arc::clone(&keep_running))?,
    ];

    if config.mpd.is_some() {
        let mpd_thread = mpd_frontend::start(config.mpd.clone(), Arc::clone(&app));
        threads.push(mpd_thread);
    }

    if config.http.is_some() {
        let http_thread = http_frontend::start(config.http.clone(), Arc::clone(&app));
        threads.push(http_thread);
    }

    for handle in threads {
        let _ = handle.join();
    }

    Ok(())
}
