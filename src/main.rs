extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate rustic_core as rustic;
extern crate rustic_mpd_frontend as mpd;
extern crate rustic_http_frontend as http;
extern crate toml;

use std::sync::{Arc, RwLock};
use std::fs::File;
use std::io::prelude::*;

#[derive(Deserialize, Clone)]
pub struct Config {
    mpd: Option<mpd::MpdConfig>,
    http: Option<http::HttpConfig>,
    pocketcasts: Option<rustic::provider::PocketcastsProvider>,
    soundcloud: Option<rustic::provider::SoundcloudProvider>
}

fn read_config() -> Config {
    let mut config_file = File::open("config.toml").unwrap();
    let mut config = String::new();
    config_file.read_to_string(&mut config).unwrap();
    toml::from_str(config.as_str()).unwrap()
}

fn main() {
    let config = read_config();
    let mut providers: rustic::provider::SharedProviders = vec![];
    
    if config.pocketcasts.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.pocketcasts.unwrap()))));
    }
    if config.soundcloud.is_some() {
        providers.push(Arc::new(RwLock::new(Box::new(config.soundcloud.unwrap()))));
    }

    let app = rustic::Rustic::new(providers);
    
    let threads = vec![
        mpd::start(config.mpd.clone(), Arc::clone(&app)),
        http::start(config.http.clone(), Arc::clone(&app)),
        rustic::sync::start(Arc::clone(&app)),
        rustic::player::start(Arc::clone(&app))
    ];

    for handle in threads {
        let _ = handle.join();
    }
}