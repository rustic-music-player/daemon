extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate rustic_core as rustic;
extern crate rustic_mpd_frontend as mpd;
extern crate rustic_http_frontend as http;
extern crate rustic_memory_store as memory_store;
extern crate rustic_sqlite_store as sqlite_store;
extern crate toml;
extern crate failure;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate ctrlc;

use std::sync::{Arc, RwLock, Condvar, Mutex};
use std::fs::File;
use std::io::prelude::*;
use failure::Error;
use memory_store::MemoryLibrary;
use sqlite_store::SqliteLibrary;

#[derive(Deserialize, Clone)]
pub struct Config {
    mpd: Option<mpd::MpdConfig>,
    http: Option<http::HttpConfig>,
    pocketcasts: Option<rustic::provider::PocketcastsProvider>,
    soundcloud: Option<rustic::provider::SoundcloudProvider>,
    spotify: Option<rustic::provider::SpotifyProvider>,
    local: Option<rustic::provider::LocalProvider>,
    library: Option<LibraryConfig>
}

#[derive(Deserialize, Clone)]
#[serde(tag = "store", rename_all = "lowercase")]
pub enum LibraryConfig {
    Memory,
    SQLite {
        path: String
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
        provider.setup()?;
    }

    let store: Box<rustic::Library> = match config.library.unwrap_or(LibraryConfig::Memory) {
        LibraryConfig::Memory => Box::new(MemoryLibrary::new()),
        LibraryConfig::SQLite { path } => Box::new(SqliteLibrary::new(path)?)
    };

    let app = rustic::Rustic::new(store, providers)?;

    let keep_running = Arc::new((Mutex::new(true), Condvar::new()));

    let interrupt = Arc::clone(&keep_running);

    ctrlc::set_handler(move || {
        info!("Shutting down");
        let &(ref lock, ref cvar) = &*interrupt;
        let mut running = lock.lock().unwrap();
        *running = false;
        cvar.notify_all();
    })?;
    
    let threads = vec![
        // mpd::start(config.mpd.clone(), Arc::clone(&app)),
        http::start(config.http.clone(), Arc::clone(&app)),
        rustic::sync::start(Arc::clone(&app), Arc::clone(&keep_running))?,
        rustic::player::start(&app, Arc::clone(&keep_running))?,
        rustic::cache::start(Arc::clone(&app), Arc::clone(&keep_running))?
    ];

    for handle in threads {
        let _ = handle.join();
    }

    Ok(())
}