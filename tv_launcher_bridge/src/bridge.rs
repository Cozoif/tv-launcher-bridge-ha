//! Everything the window and Home Assistant can ask the TV to do.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::adb::{self, Conn, Identity};
use crate::config::{AppEntry, Config, config_dir, friendly_name};
use crate::mdns;

/// Every remote button: (id used by Home Assistant, label, Android key name).
pub const KEYS: &[(&str, &str, &str)] = &[
    ("up", "Up", "DPAD_UP"),
    ("down", "Down", "DPAD_DOWN"),
    ("left", "Left", "DPAD_LEFT"),
    ("right", "Right", "DPAD_RIGHT"),
    ("ok", "OK", "DPAD_CENTER"),
    ("back", "Back", "BACK"),
    ("home", "Home", "HOME"),
    ("menu", "Menu", "MENU"),
    ("settings", "Settings", "SETTINGS"),
    ("search", "Search", "SEARCH"),
    ("assistant", "Assistant", "ASSIST"),
    ("input", "Input", "TV_INPUT"),
    ("guide", "Guide", "GUIDE"),
    ("info", "Info", "INFO"),
    ("captions", "Captions", "CAPTIONS"),
    ("play_pause", "Play/Pause", "MEDIA_PLAY_PAUSE"),
    ("play", "Play", "MEDIA_PLAY"),
    ("pause", "Pause", "MEDIA_PAUSE"),
    ("stop", "Stop", "MEDIA_STOP"),
    ("next", "Next", "MEDIA_NEXT"),
    ("previous", "Previous", "MEDIA_PREVIOUS"),
    ("rewind", "Rewind", "MEDIA_REWIND"),
    ("fast_forward", "Fast forward", "MEDIA_FAST_FORWARD"),
    ("volume_up", "Volume up", "VOLUME_UP"),
    ("volume_down", "Volume down", "VOLUME_DOWN"),
    ("mute", "Mute", "VOLUME_MUTE"),
    ("channel_up", "Channel up", "CHANNEL_UP"),
    ("channel_down", "Channel down", "CHANNEL_DOWN"),
    ("power", "Power", "POWER"),
    ("wake", "Turn on", "WAKEUP"),
    ("sleep", "Turn off", "SLEEP"),
    ("delete", "Delete", "DEL"),
    ("enter", "Enter", "ENTER"),
];

#[derive(Clone, Default)]
pub struct TvState {
    /// None = not tried yet.
    pub connected: Option<bool>,
    pub encrypted: bool,
    pub message: String,
    pub model: String,
    pub current_app: String,
    pub current_package: String,
    pub awake: Option<bool>,
    /// "playing", "paused", "buffering", "stopped" or "" (nothing playing).
    pub media_state: String,
    pub media_title: String,
    pub media_subtitle: String,
    pub media_app: String,
}

pub struct Shared {
    pub cfg: Config,
    pub log: VecDeque<String>,
    pub tv: TvState,
    pub busy: usize,
    pub api_status: String,
    pub api_running: bool,
    pub found: Vec<mdns::Found>,
}

#[derive(Default)]
struct AdbState {
    id: Option<Identity>,
    conn: Option<Conn>,
}

#[derive(Clone)]
pub struct Bridge {
    pub shared: Arc<Mutex<Shared>>,
    adb: Arc<Mutex<AdbState>>,
    repaint: Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>,
}

pub type Reply = Result<String, String>;

/// A command for an app, the way Android apps receive them (an "intent").
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct IntentReq {
    pub action: Option<String>,
    pub data: Option<String>,
    pub package: Option<String>,
    pub component: Option<String>,
    pub category: Option<String>,
    pub mime_type: Option<String>,
    pub extras: serde_json::Map<String, serde_json::Value>,
}

fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = secs % 86400;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

impl Bridge {
    pub fn new(cfg: Config) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                cfg,
                log: VecDeque::new(),
                tv: TvState::default(),
                busy: 0,
                api_status: "Not started".into(),
                api_running: false,
                found: Vec::new(),
            })),
            adb: Arc::new(Mutex::new(AdbState::default())),
            repaint: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_repaint(&self, f: Box<dyn Fn() + Send + Sync>) {
        *self.repaint.lock().unwrap() = Some(f);
    }

    fn poke(&self) {
        if let Some(f) = self.repaint.lock().unwrap().as_ref() {
            f();
        }
    }

    pub fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        println!("{msg}");
        {
            let mut s = self.shared.lock().unwrap();
            s.log.push_front(format!("{}  {}", now(), msg));
            s.log.truncate(300);
        }
        self.poke();
    }

    pub fn cfg(&self) -> Config {
        self.shared.lock().unwrap().cfg.clone()
    }

    /// Run a job in the background (used by the window).
    pub fn spawn(&self, job: impl FnOnce(&Bridge) -> Reply + Send + 'static) {
        let b = self.clone();
        self.shared.lock().unwrap().busy += 1;
        std::thread::spawn(move || {
            match job(&b) {
                Ok(m) => b.log(m),
                Err(e) => b.log(format!("Problem: {e}")),
            }
            b.shared.lock().unwrap().busy -= 1;
            b.poke();
        });
    }

    // ---------- connection ----------

    fn identity<'a>(&self, st: &'a mut AdbState) -> Result<&'a Identity, String> {
        if st.id.is_none() {
            let path = config_dir().join("adbkey.pem");
            if !path.exists() {
                self.log("Creating this computer's security key (first time only)…");
            }
            st.id = Some(adb::load_or_create_identity(&path)?);
        }
        Ok(st.id.as_ref().unwrap())
    }

    /// Forget the open connection (after changing the address, or pairing).
    pub fn disconnect(&self) {
        self.adb.lock().unwrap().conn = None;
        let mut s = self.shared.lock().unwrap();
        s.tv.connected = None;
        s.tv.message = "Disconnected; check TV status or connect to retry".into();
    }

    fn open(&self, st: &mut AdbState) -> Result<(), String> {
        let cfg = self.cfg();
        let host = cfg.tv_host.trim().to_string();
        if host.is_empty() {
            return Err("type the TV's address first, or press Find TV".into());
        }
        let me = self.clone();
        let prompt = move || {
            me.log("Look at the TV: choose \"Allow\" (tick \"Always allow\") to let this computer in.");
        };
        let id = self.identity(st)?;
        let first = adb::connect(&host, cfg.tv_port, id, &prompt);
        let conn = match first {
            Ok(c) => c,
            Err(e) if cfg.auto_find_port => {
                // Wireless debugging picks a new port after restarts: look for it.
                let found = mdns::discover(Duration::from_millis(2500));
                let hit = found
                    .iter()
                    .find(|f| f.kind == mdns::Kind::Connect && f.ip == host && f.port != cfg.tv_port);
                match hit {
                    Some(f) => {
                        self.log(format!("The TV's wireless debugging port changed to {}; using it.", f.port));
                        {
                            let mut s = self.shared.lock().unwrap();
                            s.cfg.tv_port = f.port;
                            s.cfg.save();
                        }
                        adb::connect(&host, f.port, id, &prompt)?
                    }
                    None => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };
        {
            let mut s = self.shared.lock().unwrap();
            s.tv.encrypted = conn.encrypted;
            if !conn.device.is_empty() {
                s.tv.model = conn.device.clone();
            }
        }
        st.conn = Some(conn);
        Ok(())
    }

    pub fn shell(&self, command: &str) -> Reply {
        let mut st = self.adb.lock().unwrap();
        let mut result = Err(String::new());
        for attempt in 0..2 {
            if st.conn.is_none() {
                if let Err(e) = self.open(&mut st) {
                    result = Err(e);
                    break;
                }
            }
            result = st.conn.as_mut().unwrap().shell(command);
            match &result {
                Ok(_) => break,
                Err(_) => {
                    // The TV may have dropped an idle connection; try once more fresh.
                    st.conn = None;
                    if attempt == 1 {
                        break;
                    }
                }
            }
        }
        drop(st);
        let cfg = self.cfg();
        {
            let mut s = self.shared.lock().unwrap();
            match &result {
                Ok(_) => {
                    s.tv.connected = Some(true);
                    s.tv.message = format!(
                        "Connected to {}:{}{}",
                        cfg.tv_host.trim(),
                        s.cfg.tv_port,
                        if s.tv.encrypted { " (wireless debugging, encrypted)" } else { "" }
                    );
                }
                Err(e) => {
                    s.tv.connected = Some(false);
                    s.tv.message = e.clone();
                }
            }
        }
        self.poke();
        result
    }

    pub fn pair(&self, host: &str, port: u16, code: &str) -> Reply {
        let mut st = self.adb.lock().unwrap();
        let id = self.identity(&mut st)?;
        let name = adb::pair(host.trim(), port, code, id)?;
        st.conn = None;
        drop(st);
        let mut s = self.shared.lock().unwrap();
        s.cfg.tv_host = host.trim().to_string();
        s.cfg.paired_with = name.clone();
        s.cfg.save();
        Ok(format!(
            "Paired with {name}. Now press Connect (use the port shown next to \"IP address & Port\")."
        ))
    }

    pub fn find_tvs(&self) -> Reply {
        let found = mdns::discover(Duration::from_millis(3000));
        let n = found.len();
        let host = self.cfg().tv_host;
        let mut msg = if n == 0 {
            "No TV answered. Make sure Wireless debugging is on and the TV is on the same network.".to_string()
        } else {
            format!("Found {n} wireless debugging service(s)")
        };
        // Fill in the address/port automatically when it's clear which TV it is.
        let connect: Vec<&mdns::Found> = found.iter().filter(|f| f.kind == mdns::Kind::Connect).collect();
        let pick = connect
            .iter()
            .find(|f| f.ip == host.trim())
            .or_else(|| if host.trim().is_empty() && connect.len() == 1 { connect.first() } else { None });
        let pairing = found.iter().find(|f| f.kind == mdns::Kind::Pairing);
        {
            let mut s = self.shared.lock().unwrap();
            if let Some(f) = pick {
                s.cfg.tv_host = f.ip.clone();
                s.cfg.tv_port = f.port;
                msg = format!("Found the TV at {}:{}", f.ip, f.port);
            }
            if let Some(p) = pairing {
                s.cfg.pair_port = p.port;
                if s.cfg.tv_host.trim().is_empty() {
                    s.cfg.tv_host = p.ip.clone();
                }
                msg.push_str(&format!(" (pairing screen open, pairing port {})", p.port));
            }
            s.cfg.save();
            s.found = found;
        }
        self.disconnect();
        self.poke();
        Ok(msg)
    }

    pub fn test_connection(&self) -> Reply {
        let model = self.shell("getprop ro.product.model")?;
        let version = self.shell("getprop ro.build.version.release").unwrap_or_default();
        let _ = self.refresh_status();
        Ok(format!("Connected to {} (Android {})", model.trim(), version.trim()))
    }

    // ---------- apps ----------

    /// Find an app by its name ("jellyfin") or package ("org.jellyfin.androidtv").
    pub fn resolve_app(&self, what: &str) -> Option<AppEntry> {
        let w = what.trim();
        let cfg = self.cfg();
        if adb::is_package_name(w) {
            return Some(cfg.apps.iter().find(|a| a.package == w).cloned().unwrap_or(AppEntry {
                name: friendly_name(w),
                package: w.into(),
            }));
        }
        let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
        let nw = norm(w);
        if nw.is_empty() {
            return None;
        }
        let known = crate::config::KNOWN_APPS.iter().map(|(n, p)| AppEntry {
            name: n.to_string(),
            package: p.to_string(),
        });
        let all: Vec<AppEntry> = cfg.apps.iter().cloned().chain(known).collect();
        all.iter()
            .find(|a| norm(&a.name) == nw)
            .or_else(|| all.iter().find(|a| norm(&a.name).starts_with(&nw)))
            .or_else(|| all.iter().find(|a| norm(&a.name).contains(&nw)))
            .or_else(|| all.iter().find(|a| a.package.to_lowercase().contains(&nw)))
            .cloned()
    }

    pub fn launch(&self, what: &str) -> Reply {
        let app = self
            .resolve_app(what)
            .ok_or_else(|| format!("I don't know an app called \"{what}\". Add it in the Apps list."))?;
        let out = self.shell(&format!(
            "monkey -p {} -c android.intent.category.LEANBACK_LAUNCHER 1",
            app.package
        ))?;
        if out.contains("No activities found") || out.contains("aborted") {
            let out2 = self.shell(&format!("monkey -p {} -c android.intent.category.LAUNCHER 1", app.package))?;
            if out2.contains("No activities found") {
                return Err(format!("{} doesn't seem to be installed on the TV", app.name));
            }
        }
        Ok(format!("Opened {}", app.name))
    }

    pub fn close_app(&self, what: &str) -> Reply {
        let app = self
            .resolve_app(what)
            .ok_or_else(|| format!("I don't know an app called \"{what}\""))?;
        self.shell(&format!("am force-stop {}", app.package))?;
        Ok(format!("Closed {}", app.name))
    }

    pub fn list_apps(&self) -> Result<Vec<AppEntry>, String> {
        let out = self.shell(
            "cmd package query-activities --brief -a android.intent.action.MAIN -c android.intent.category.LEANBACK_LAUNCHER",
        )?;
        let mut pkgs: Vec<String> = out
            .split_whitespace()
            .filter_map(|t| t.split_once('/').map(|(p, _)| p.to_string()))
            .filter(|p| adb::is_package_name(p))
            .collect();
        if pkgs.is_empty() {
            let out = self.shell("pm list packages -3")?;
            pkgs = out
                .lines()
                .filter_map(|l| l.trim().strip_prefix("package:").map(|s| s.to_string()))
                .collect();
        }
        pkgs.sort();
        pkgs.dedup();
        let mut apps: Vec<AppEntry> = pkgs
            .into_iter()
            .map(|p| AppEntry {
                name: friendly_name(&p),
                package: p,
            })
            .collect();
        apps.sort_by_key(|a| a.name.to_lowercase());
        Ok(apps)
    }

    /// Replace the app list with what's installed on the TV (keeping renamed entries).
    pub fn load_apps_from_tv(&self) -> Reply {
        let found = self.list_apps()?;
        let n = found.len();
        let mut s = self.shared.lock().unwrap();
        let old = s.cfg.apps.clone();
        s.cfg.apps = found
            .into_iter()
            .map(|a| old.iter().find(|o| o.package == a.package).cloned().unwrap_or(a))
            .collect();
        s.cfg.save();
        Ok(format!("Found {n} apps on the TV"))
    }

    // ---------- buttons ----------

    pub fn keycode(name: &str) -> Option<String> {
        let n = name.trim().to_lowercase().replace(['-', ' '], "_");
        if let Some((_, _, k)) = KEYS.iter().find(|(id, _, _)| *id == n) {
            return Some(format!("KEYCODE_{k}"));
        }
        let alias = match n.as_str() {
            "select" | "center" => Some("DPAD_CENTER"),
            "playpause" => Some("MEDIA_PLAY_PAUSE"),
            "forward" | "ff" => Some("MEDIA_FAST_FORWARD"),
            "prev" => Some("MEDIA_PREVIOUS"),
            "vol_up" => Some("VOLUME_UP"),
            "vol_down" => Some("VOLUME_DOWN"),
            "on" => Some("WAKEUP"),
            "off" => Some("SLEEP"),
            _ => None,
        };
        if let Some(a) = alias {
            return Some(format!("KEYCODE_{a}"));
        }
        let up = n.to_uppercase();
        let code = if up.starts_with("KEYCODE_") { up } else { format!("KEYCODE_{up}") };
        code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_').then_some(code)
    }

    pub fn key(&self, name: &str, long_press: bool) -> Reply {
        let code = Self::keycode(name).ok_or_else(|| format!("unknown button \"{name}\""))?;
        let lp = if long_press { "--longpress " } else { "" };
        self.shell(&format!("input keyevent {lp}{code}"))?;
        Ok(format!(
            "Pressed {}{}",
            code.trim_start_matches("KEYCODE_").replace('_', " ").to_lowercase(),
            if long_press { " (held)" } else { "" }
        ))
    }

    pub fn power(&self, on: bool) -> Reply {
        self.shell(if on { "input keyevent KEYCODE_WAKEUP" } else { "input keyevent KEYCODE_SLEEP" })?;
        self.shared.lock().unwrap().tv.awake = Some(on);
        Ok(if on { "Woke the TV up".into() } else { "Put the TV to sleep".into() })
    }

    // ---------- commands for apps ----------

    /// Open a link (a deep link or web address), optionally inside a given app.
    pub fn open_link(&self, url: &str, app: Option<&str>) -> Reply {
        let url = url.trim();
        if url.is_empty() {
            return Err("the link is empty".into());
        }
        let mut cmd = format!("am start -a android.intent.action.VIEW -d {}", adb::sh_quote(url));
        let mut label = String::new();
        if let Some(a) = app.filter(|a| !a.trim().is_empty()) {
            let e = self.resolve_app(a).ok_or_else(|| format!("I don't know an app called \"{a}\""))?;
            cmd.push_str(&format!(" -p {}", e.package));
            label = format!(" in {}", e.name);
        }
        let out = self.shell(&cmd)?;
        if out.contains("Error") {
            return Err(format!("the TV couldn't open that link{label}: {}", out.trim()));
        }
        Ok(format!("Opened the link{label}"))
    }

    /// Search the whole TV, or inside one app.
    pub fn search(&self, query: &str, app: Option<&str>) -> Reply {
        let q = query.trim();
        if q.is_empty() {
            return Err("the search is empty".into());
        }
        match app.filter(|a| !a.trim().is_empty()) {
            Some(a) => {
                let e = self.resolve_app(a).ok_or_else(|| format!("I don't know an app called \"{a}\""))?;
                let out = self.shell(&format!(
                    "am start -a android.intent.action.SEARCH --es query {} -p {}",
                    adb::sh_quote(q),
                    e.package
                ))?;
                if out.contains("Error") {
                    // App has no search screen of its own: open it and type instead.
                    self.launch(&e.package)?;
                    std::thread::sleep(Duration::from_secs(3));
                    self.key("search", false)?;
                    std::thread::sleep(Duration::from_millis(800));
                    self.type_text(q)?;
                }
                Ok(format!("Searching {} for \"{q}\"", e.name))
            }
            None => {
                let out = self.shell(&format!(
                    "am start -a android.search.action.GLOBAL_SEARCH --es query {}",
                    adb::sh_quote(q)
                ))?;
                if out.contains("Error") {
                    return Err(format!("the TV couldn't search: {}", out.trim()));
                }
                Ok(format!("Searching the TV for \"{q}\""))
            }
        }
    }

    /// Send any intent to an app (advanced: deep links, actions, extras).
    pub fn send_intent(&self, req: &IntentReq) -> Reply {
        let mut cmd = String::from("am start");
        let mut add = |flag: &str, v: &Option<String>| -> Result<(), String> {
            if let Some(v) = v.as_ref().filter(|v| !v.trim().is_empty()) {
                cmd.push_str(&format!(" {flag} {}", adb::sh_quote(v.trim())));
            }
            Ok(())
        };
        add("-a", &req.action)?;
        add("-d", &req.data)?;
        add("-t", &req.mime_type)?;
        add("-c", &req.category)?;
        add("-n", &req.component)?;
        if let Some(p) = req.package.as_ref().filter(|p| !p.trim().is_empty()) {
            let e = self.resolve_app(p).ok_or_else(|| format!("I don't know an app called \"{p}\""))?;
            cmd.push_str(&format!(" -p {}", e.package));
        }
        for (k, v) in &req.extras {
            let key = adb::sh_quote(k);
            match v {
                serde_json::Value::Bool(b) => cmd.push_str(&format!(" --ez {key} {b}")),
                serde_json::Value::Number(n) if n.is_i64() => {
                    cmd.push_str(&format!(" --el {key} {}", n.as_i64().unwrap_or(0)))
                }
                serde_json::Value::Number(n) => cmd.push_str(&format!(" --ef {key} {}", n.as_f64().unwrap_or(0.0))),
                serde_json::Value::String(s) => cmd.push_str(&format!(" --es {key} {}", adb::sh_quote(s))),
                other => cmd.push_str(&format!(" --es {key} {}", adb::sh_quote(&other.to_string()))),
            }
        }
        if cmd == "am start" {
            return Err("give at least an action, a link, a package or a component".into());
        }
        let out = self.shell(&cmd)?;
        if out.contains("Error") {
            return Err(format!("the TV refused the command: {}", out.trim()));
        }
        Ok("Sent the command to the app".into())
    }

    pub fn type_text(&self, text: &str) -> Reply {
        let t: String = text.chars().filter(|c| !c.is_control()).collect();
        let t = t.replace(' ', "%s");
        self.shell(&format!("input text {}", adb::sh_quote(&t)))?;
        Ok("Typed the text on the TV".into())
    }

    /// Any ADB shell command (only when allowed in the settings).
    pub fn raw(&self, command: &str) -> Reply {
        if !self.cfg().allow_raw_commands {
            return Err("raw ADB commands are switched off in TV Launcher Bridge (Home Assistant link settings)".into());
        }
        self.shell(command).map(|o| o.trim().to_string())
    }

    // ---------- status ----------

    pub fn refresh_status(&self) -> Reply {
        let out = self.shell(
            "echo __TLB_A__; dumpsys activity activities | grep -E 'mResumedActivity|topResumedActivity' | head -n 1; \
             echo __TLB_P__; dumpsys power | grep -m1 mWakefulness=; \
             echo __TLB_M__; dumpsys media_session | grep -E 'package=|active=|state=PlaybackState|description='; \
             echo __TLB_D__; getprop ro.product.model",
        )?;
        let p = parse_status(&out);
        let name = if p.current.is_empty() {
            String::new()
        } else {
            self.resolve_app(&p.current).map(|a| a.name).unwrap_or(p.current.clone())
        };
        let awake = p.awake;
        {
            let mut s = self.shared.lock().unwrap();
            s.tv.current_app = name.clone();
            s.tv.current_package = p.current;
            s.tv.awake = p.awake;
            if !p.model.is_empty() {
                s.tv.model = p.model;
            }
            s.tv.media_state = p.media_state;
            s.tv.media_title = p.media_title;
            s.tv.media_subtitle = p.media_subtitle;
            s.tv.media_app = p.media_app;
        }
        self.poke();
        Ok(format!(
            "TV is {}{}",
            match awake {
                Some(true) => "on",
                Some(false) => "asleep",
                None => "reachable",
            },
            if name.is_empty() { String::new() } else { format!(", showing {name}") }
        ))
    }

    pub fn status_json(&self) -> serde_json::Value {
        let s = self.shared.lock().unwrap();
        serde_json::json!({
            "connected": s.tv.connected.unwrap_or(false),
            "encrypted": s.tv.encrypted,
            "awake": s.tv.awake,
            "model": s.tv.model,
            "current_app": s.tv.current_app,
            "current_package": s.tv.current_package,
            "media_state": s.tv.media_state,
            "media_title": s.tv.media_title,
            "media_subtitle": s.tv.media_subtitle,
            "media_app": s.tv.media_app,
            "message": s.tv.message,
            "tv_host": s.cfg.tv_host,
        })
    }

    // ---------- Jellyfin: play a named show or film ----------

    fn jellyfin(&self) -> Result<(Jellyfin, String), String> {
        let cfg = self.cfg();
        let base = cfg.jellyfin_url.trim().trim_end_matches('/').to_string();
        if base.is_empty() || cfg.jellyfin_api_key.trim().is_empty() {
            return Err("fill in the Jellyfin server address and API key first".into());
        }
        Ok((Jellyfin { base, key: cfg.jellyfin_api_key.trim().to_string() }, cfg.jellyfin_device))
    }

    pub fn jellyfin_play(&self, show: &str, season: Option<u32>, episode: Option<u32>) -> Reply {
        let (jf, device) = self.jellyfin()?;
        let session = jf.find_tv_session(&device, false)?;
        let user = session["UserId"].as_str().unwrap_or("");
        let item = jf.find_item(show, user)?;
        let id = item["Id"].as_str().unwrap_or("");
        let (play_id, what) = if item["Type"].as_str() == Some("Series") {
            jf.pick_episode(id, user, season, episode)?
        } else {
            (id.to_string(), item["Name"].as_str().unwrap_or(show).to_string())
        };
        jf.play(&session, &play_id, &what, 0)
    }

    /// The whole routine: wake the TV, open Jellyfin, play the show.
    pub fn watch(&self, show: &str, season: Option<u32>, episode: Option<u32>) -> Reply {
        self.with_jellyfin_on_tv(|| self.jellyfin_play(show, season, episode))
    }

    pub fn watch_sitcom(&self, ask: bool, show: Option<&str>) -> Result<(String, Vec<String>), String> {
        self.with_jellyfin_on_tv(|| {
            let (jf, device) = self.jellyfin()?;
            let session = jf.find_tv_session(&device, true)?;
            let user = session["UserId"].as_str().unwrap_or("");
            let options = jf.sitcom_options(user, if ask || show.is_some() { usize::MAX } else { 1 })?;
            if ask && show.is_none() && options.len() > 1 {
                return Ok(("Choose a sitcom".into(), options.into_iter().map(|option| option.series_name).collect()));
            }
            let selected = if let Some(show) = show {
                let mut matches = options.iter().filter(|option| {
                    option.series_name.to_lowercase() == show.trim().to_lowercase()
                });
                let selected = matches.next().ok_or("That sitcom is no longer in the current choices")?;
                if matches.next().is_some() {
                    return Err("More than one series has that title".into());
                }
                selected
            } else {
                options.first().ok_or("no series tagged Sitcom in Continue Watching or Next Up")?
            };
            let message = jf.play(&session, &selected.episode_id, &selected.label, selected.position)?;
            Ok((message, Vec::new()))
        })
    }

    fn with_jellyfin_on_tv<T>(&self, play: impl Fn() -> Result<T, String>) -> Result<T, String> {
        let _ = self.power(true);
        let in_jellyfin = self
            .shell("dumpsys activity activities | grep -E 'mResumedActivity|topResumedActivity' | head -n 1")
            .map(|o| o.contains("org.jellyfin"))
            .unwrap_or(false);
        if !in_jellyfin {
            self.launch("org.jellyfin.androidtv")?;
            let wait = self.cfg().jellyfin_wait_s.clamp(1, 30);
            std::thread::sleep(Duration::from_secs(wait));
        }
        let mut last = String::new();
        for attempt in 0..4 {
            match play() {
                Ok(m) => return Ok(m),
                Err(e) if e.contains("no Jellyfin app") && attempt < 3 => {
                    last = e;
                    std::thread::sleep(Duration::from_secs(3));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    pub fn jellyfin_test(&self) -> Reply {
        let cfg = self.cfg();
        let jf = Jellyfin {
            base: cfg.jellyfin_url.trim().trim_end_matches('/').to_string(),
            key: cfg.jellyfin_api_key.trim().to_string(),
        };
        let info = jf.get("/System/Info")?;
        let name = info["ServerName"].as_str().unwrap_or("Jellyfin");
        let sessions = jf.get("/Sessions")?;
        let tvs: Vec<String> = sessions
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|s| s["SupportsRemoteControl"].as_bool().unwrap_or(false))
                    .map(|s| {
                        format!(
                            "{} ({})",
                            s["DeviceName"].as_str().unwrap_or("?"),
                            s["Client"].as_str().unwrap_or("?")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(format!(
            "Reached server \"{name}\". Players that can be controlled right now: {}",
            if tvs.is_empty() { "none (open Jellyfin on the TV)".to_string() } else { tvs.join(", ") }
        ))
    }
}

#[derive(Default, Debug)]
pub struct ParsedStatus {
    pub current: String,
    pub awake: Option<bool>,
    pub model: String,
    pub media_state: String,
    pub media_title: String,
    pub media_subtitle: String,
    pub media_app: String,
}

/// Read the TV's answer to the status command (sections start with __TLB_x__).
pub fn parse_status(out: &str) -> ParsedStatus {
    let section = |tag: &str| -> String {
        let marker = format!("__TLB_{tag}__");
        out.split_once(&marker)
            .map(|(_, rest)| rest.split("__TLB_").next().unwrap_or("").to_string())
            .unwrap_or_default()
    };
    let mut p = ParsedStatus::default();
    for tok in section("A").split_whitespace() {
        if let Some((pkg, _)) = tok.split_once('/') {
            if adb::is_package_name(pkg) {
                p.current = pkg.to_string();
                break;
            }
        }
    }
    let power = section("P");
    p.awake = if power.contains("mWakefulness=Awake") {
        Some(true)
    } else if power.contains("mWakefulness=") {
        Some(false)
    } else {
        None
    };
    // Media sessions: package, active flag, playback state, description.
    #[derive(Default)]
    struct Sess {
        pkg: String,
        active: bool,
        state: u32,
        desc: String,
    }
    let mut sessions: Vec<Sess> = Vec::new();
    for line in section("M").lines() {
        let l = line.trim();
        if let Some(pkg) = l.strip_prefix("package=") {
            sessions.push(Sess {
                pkg: pkg.trim().to_string(),
                ..Default::default()
            });
        } else if let Some(s) = sessions.last_mut() {
            if let Some(a) = l.strip_prefix("active=") {
                s.active = a.trim() == "true";
            } else if let Some(i) = l.find("state=PlaybackState {state=") {
                let rest = &l[i + 27..];
                s.state = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            } else if let Some(i) = l.find("description=") {
                s.desc = l[i + 12..].trim().to_string();
            }
        }
    }
    let playing = |st: u32| matches!(st, 2 | 3 | 6);
    let best = sessions
        .iter()
        .find(|s| s.active && playing(s.state))
        .or_else(|| sessions.iter().find(|s| playing(s.state)));
    if let Some(m) = best {
        p.media_state = match m.state {
            3 => "playing",
            2 => "paused",
            _ => "buffering",
        }
        .into();
        let parts: Vec<&str> = m
            .desc
            .split(", ")
            .map(|x| x.trim())
            .filter(|x| *x != "null" && !x.is_empty())
            .collect();
        p.media_title = parts.first().map(|x| x.to_string()).unwrap_or_default();
        p.media_subtitle = parts.get(1).map(|x| x.to_string()).unwrap_or_default();
        p.media_app = m.pkg.clone();
    }
    p.model = section("D").trim().to_string();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tv_status() {
        let out = "__TLB_A__\n  mResumedActivity: ActivityRecord{1a2b u0 org.jellyfin.androidtv/.ui.MainActivity t42}\n\
__TLB_P__\n  mWakefulness=Awake\n\
__TLB_M__\n    package=com.google.android.youtube.tv\n    active=false\n    state=PlaybackState {state=2, position=0, speed=0.0}\n\
      metadata: size=0, description=null\n\
    package=org.jellyfin.androidtv\n    active=true\n    state=PlaybackState {state=3, position=81000, speed=1.0}\n\
      metadata: size=5, description=The Pool @ Home, Bluey, null\n\
__TLB_D__\nChromecast\n";
        let p = parse_status(out);
        assert_eq!(p.current, "org.jellyfin.androidtv");
        assert_eq!(p.awake, Some(true));
        assert_eq!(p.media_state, "playing");
        assert_eq!(p.media_title, "The Pool @ Home");
        assert_eq!(p.media_subtitle, "Bluey");
        assert_eq!(p.media_app, "org.jellyfin.androidtv");
        assert_eq!(p.model, "Chromecast");
        let off = parse_status("__TLB_A__\n__TLB_P__\nmWakefulness=Asleep\n__TLB_M__\n__TLB_D__\nChromecast");
        assert_eq!(off.awake, Some(false));
        assert_eq!(off.media_state, "");
    }
}

struct SitcomOption {
    series_id: String,
    series_name: String,
    episode_id: String,
    label: String,
    position: u64,
}

struct Jellyfin {
    base: String,
    key: String,
}

pub fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

impl Jellyfin {
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into()
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}{}", self.base, path);
        let mut res = Self::agent()
            .get(&url)
            .header("X-Emby-Token", &self.key)
            .call()
            .map_err(|e| format!("Jellyfin server: {e}"))?;
        let body = res.body_mut().read_to_string().map_err(|e| format!("Jellyfin server: {e}"))?;
        serde_json::from_str(&body).map_err(|e| format!("Jellyfin server sent something odd: {e}"))
    }

    fn post(&self, path: &str) -> Result<(), String> {
        let url = format!("{}{}", self.base, path);
        Self::agent()
            .post(&url)
            .header("X-Emby-Token", &self.key)
            .send_empty()
            .map_err(|e| format!("Jellyfin server: {e}"))?;
        Ok(())
    }

    fn play(&self, session: &serde_json::Value, id: &str, label: &str, position: u64) -> Reply {
        let sid = session["Id"].as_str().unwrap_or("");
        let mut path = format!("/Sessions/{sid}/Playing?playCommand=PlayNow&itemIds={id}");
        if position > 0 {
            path.push_str(&format!("&startPositionTicks={position}"));
        }
        self.post(&path)?;
        Ok(format!("Playing {label} on Jellyfin"))
    }

    fn find_tv_session(&self, device: &str, require_tv: bool) -> Result<serde_json::Value, String> {
        let sessions = self.get("/Sessions")?;
        let list = sessions.as_array().cloned().unwrap_or_default();
        let controllable: Vec<&serde_json::Value> = list
            .iter()
            .filter(|s| s["SupportsRemoteControl"].as_bool().unwrap_or(false))
            .collect();
        let dev = device.trim().to_lowercase();
        let pick = if !dev.is_empty() {
            controllable
                .iter()
                .find(|s| s["DeviceName"].as_str().unwrap_or("").to_lowercase().contains(&dev))
        } else {
            controllable
                .iter()
                .find(|s| s["Client"].as_str().unwrap_or("").to_lowercase().contains("android tv"))
                .or_else(|| if require_tv { None } else { controllable.first() })
        };
        pick.map(|s| (*s).clone())
            .ok_or_else(|| "no Jellyfin app is open and signed in on the TV yet".to_string())
    }

    fn find_item(&self, name: &str, user: &str) -> Result<serde_json::Value, String> {
        let q = format!(
            "/Items?searchTerm={}&IncludeItemTypes=Series,Movie,Episode,MusicAlbum&Recursive=true&Limit=10{}",
            url_encode(name.trim()),
            if user.is_empty() { String::new() } else { format!("&userId={user}") }
        );
        let res = self.get(&q)?;
        let items = res["Items"].as_array().cloned().unwrap_or_default();
        let want = name.trim().to_lowercase();
        items
            .iter()
            .find(|i| i["Name"].as_str().unwrap_or("").to_lowercase() == want)
            .or_else(|| items.iter().find(|i| i["Type"].as_str() == Some("Series")))
            .or_else(|| items.first())
            .cloned()
            .ok_or_else(|| format!("couldn't find \"{}\" in your Jellyfin library", name.trim()))
    }

    fn episode_label(episode: &serde_json::Value) -> String {
        format!(
            "{} S{}E{} \"{}\"",
            episode["SeriesName"].as_str().unwrap_or(""),
            episode["ParentIndexNumber"].as_u64().unwrap_or(0),
            episode["IndexNumber"].as_u64().unwrap_or(0),
            episode["Name"].as_str().unwrap_or("")
        )
    }

    fn sitcom_options(&self, user: &str, limit: usize) -> Result<Vec<SitcomOption>, String> {
        if user.is_empty() {
            return Err("the TV Jellyfin session has no signed-in user".into());
        }
        const PAGE_SIZE: usize = 30;
        let user = url_encode(user);
        let queues = [
            ("/UserItems/Resume?includeItemTypes=Episode", true),
            ("/Shows/NextUp?disableFirstEpisode=true&enableResumable=false", false),
        ];

        let mut options: Vec<SitcomOption> = Vec::new();
        for (queue, resume) in queues {
            let mut offset = 0;
            loop {
                let response = self.get(&format!(
                    "{queue}&userId={user}&limit={PAGE_SIZE}&startIndex={offset}&enableImages=false"
                ))?;
                let episodes = response["Items"].as_array()
                    .ok_or("Jellyfin did not return an episode list")?;
                let mut series_ids: Vec<_> = episodes.iter()
                    .filter_map(|episode| episode["SeriesId"].as_str())
                    .filter(|id| !id.is_empty())
                    .collect();
                series_ids.sort_unstable();
                series_ids.dedup();
                if !series_ids.is_empty() {
                    let ids = url_encode(&series_ids.join(","));
                    let series = self.get(&format!(
                        "/Items?userId={user}&ids={ids}&recursive=true&includeItemTypes=Series&fields=Genres,Tags&enableImages=false"
                    ))?;
                    let sitcom_ids: Vec<_> = series["Items"].as_array()
                        .ok_or("Jellyfin did not return series metadata")?
                        .iter()
                        .filter(|item| ["Genres", "Tags"].iter().any(|field| {
                            item[*field].as_array().into_iter().flatten().any(|value| {
                                value.as_str().is_some_and(|label| label.trim().eq_ignore_ascii_case("sitcom"))
                            })
                        }))
                        .filter_map(|item| item["Id"].as_str())
                        .collect();
                    for episode in episodes {
                        let Some(id) = episode["Id"].as_str().filter(|id| !id.is_empty()) else {
                            continue;
                        };
                        let Some(series_id) = episode["SeriesId"].as_str() else {
                            continue;
                        };
                        let Some(series_name) = episode["SeriesName"].as_str().filter(|name| !name.is_empty()) else {
                            continue;
                        };
                        let position = episode["UserData"]["PlaybackPositionTicks"].as_u64().unwrap_or(0);
                        if (resume && position == 0) || !sitcom_ids.contains(&series_id)
                            || options.iter().any(|option| option.series_id == series_id)
                        {
                            continue;
                        }
                        options.push(SitcomOption {
                            series_id: series_id.to_string(),
                            series_name: series_name.to_string(),
                            episode_id: id.to_string(),
                            label: Self::episode_label(episode),
                            position: if resume { position } else { 0 },
                        });
                        if options.len() >= limit {
                            return Ok(options);
                        }
                    }
                }
                if episodes.len() < PAGE_SIZE {
                    break;
                }
                offset += episodes.len();
            }
            if !options.is_empty() {
                break;
            }
        }
        Ok(options)
    }

    fn pick_episode(
        &self,
        series: &str,
        user: &str,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<(String, String), String> {
        let uq = if user.is_empty() { String::new() } else { format!("&userId={user}") };
        if season.is_none() && episode.is_none() {
            let next = self.get(&format!("/Shows/NextUp?seriesId={series}&Limit=1{uq}"))?;
            if let Some(e) = next["Items"].as_array().and_then(|a| a.first()) {
                return Ok((e["Id"].as_str().unwrap_or("").to_string(), Self::episode_label(e)));
            }
        }
        let mut q = format!("/Shows/{series}/Episodes?{}", uq.trim_start_matches('&'));
        if let Some(s) = season {
            q.push_str(&format!("&season={s}"));
        }
        let eps = self.get(&q)?;
        let list = eps["Items"].as_array().cloned().unwrap_or_default();
        let e = match episode {
            Some(n) => list.iter().find(|e| e["IndexNumber"].as_u64() == Some(n as u64)),
            None => list.first(),
        }
        .ok_or_else(|| "couldn't find that episode".to_string())?;
        Ok((e["Id"].as_str().unwrap_or("").to_string(), Self::episode_label(e)))
    }
}
