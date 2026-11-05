use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEntry {
    pub name: String,
    pub package: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub tv_host: String,
    /// The connect port from "Wireless debugging › IP address & Port" (5555 for plain debugging).
    pub tv_port: u16,
    /// The port shown under the pairing code (only while that screen is open).
    pub pair_port: u16,
    /// Name the TV gave when we paired (empty = never paired).
    pub paired_with: String,
    /// Look for the new port automatically when the old one stops answering.
    pub auto_find_port: bool,
    /// Let Home Assistant send any ADB shell command.
    pub allow_raw_commands: bool,
    pub api_port: u16,
    pub api_token: String,
    pub jellyfin_url: String,
    pub jellyfin_api_key: String,
    /// Part of the TV's device name as Jellyfin shows it (empty = pick the TV automatically).
    pub jellyfin_device: String,
    /// Seconds to wait after opening Jellyfin before asking it to play.
    pub jellyfin_wait_s: u64,
    pub apps: Vec<AppEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tv_host: String::new(),
            tv_port: 5555,
            pair_port: 0,
            paired_with: String::new(),
            auto_find_port: true,
            allow_raw_commands: false,
            api_port: 8125,
            api_token: String::new(),
            jellyfin_url: String::new(),
            jellyfin_api_key: String::new(),
            jellyfin_device: String::new(),
            jellyfin_wait_s: 6,
            apps: default_apps(),
        }
    }
}

/// Well-known TV apps. Names only, the person can add any other app.
pub const KNOWN_APPS: &[(&str, &str)] = &[
    ("Jellyfin", "org.jellyfin.androidtv"),
    ("YouTube", "com.google.android.youtube.tv"),
    ("Netflix", "com.netflix.ninja"),
    ("Plex", "com.plexapp.android"),
    ("Kodi", "org.xbmc.kodi"),
    ("Spotify", "com.spotify.tv.android"),
    ("Prime Video", "com.amazon.amazonvideo.livingroom"),
    ("Disney+", "com.disney.disneyplus"),
    ("Max", "com.wbd.stream"),
    ("Twitch", "tv.twitch.android.app"),
    ("YouTube Music", "com.google.android.youtube.tvmusic"),
    ("Apple TV", "com.apple.atve.androidtv.appletv"),
    ("VLC", "org.videolan.vlc"),
    ("Play Store", "com.android.vending"),
    ("Settings", "com.android.tv.settings"),
];

fn default_apps() -> Vec<AppEntry> {
    KNOWN_APPS
        .iter()
        .take(10)
        .chain(KNOWN_APPS.iter().filter(|a| a.0 == "Settings"))
        .map(|(n, p)| AppEntry {
            name: n.to_string(),
            package: p.to_string(),
        })
        .collect()
}

pub fn friendly_name(package: &str) -> String {
    if let Some((n, _)) = KNOWN_APPS.iter().find(|(_, p)| *p == package) {
        return n.to_string();
    }
    let parts: Vec<&str> = package.split('.').collect();
    let skip = ["com", "org", "tv", "android", "app", "net", "google", "androidtv", "atv", "livingroom"];
    let pick = parts
        .iter()
        .rev()
        .find(|p| !skip.contains(p))
        .or(parts.last())
        .copied()
        .unwrap_or(package);
    let mut c = pick.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => package.to_string(),
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("tv-launcher-bridge")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(config_dir().join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("settings.json"), s);
        }
    }
}
