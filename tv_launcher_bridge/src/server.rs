//! The little web address Home Assistant calls (no MQTT needed).

use std::sync::{Arc, Mutex};

use crate::bridge::{Bridge, Reply};

pub struct ApiServer {
    server: Mutex<Option<Arc<tiny_http::Server>>>,
}

impl ApiServer {
    pub fn new() -> Self {
        Self {
            server: Mutex::new(None),
        }
    }

    pub fn stop(&self, bridge: &Bridge) {
        if let Some(s) = self.server.lock().unwrap().take() {
            s.unblock();
        }
        let mut sh = bridge.shared.lock().unwrap();
        sh.api_running = false;
        sh.api_status = "Stopped".into();
    }

    pub fn start(&self, bridge: &Bridge) {
        self.stop(bridge);
        let port = bridge.cfg().api_port;
        match tiny_http::Server::http((std::env::var("TV_BRIDGE_BIND").unwrap_or_else(|_| "0.0.0.0".into()), port)) {
            Ok(s) => {
                let s = Arc::new(s);
                *self.server.lock().unwrap() = Some(s.clone());
                {
                    let mut sh = bridge.shared.lock().unwrap();
                    sh.api_running = true;
                    sh.api_status = format!("Listening on port {port}");
                }
                bridge.log(format!("Home Assistant link is listening on port {port}"));
                let b = bridge.clone();
                std::thread::spawn(move || {
                    for req in s.incoming_requests() {
                        let b = b.clone();
                        std::thread::spawn(move || handle(&b, req));
                    }
                });
            }
            Err(e) => {
                let mut sh = bridge.shared.lock().unwrap();
                sh.api_running = false;
                sh.api_status = format!("Couldn't use port {port}: {e}");
            }
        }
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (decode(k) == name).then(|| decode(v))
    })
}

fn json_reply(ok: bool, message: &str, extra: Option<serde_json::Value>) -> String {
    let mut v = serde_json::json!({ "ok": ok, "message": message });
    if let Some(serde_json::Value::Object(m)) = extra {
        for (k, val) in m {
            v[k] = val;
        }
    }
    v.to_string()
}

fn handle(b: &Bridge, mut req: tiny_http::Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.trim_end_matches('/').to_string();
    let query = query.to_string();

    // Optional JSON body: {"show": "...", "app": "..."} etc.
    let mut body = String::new();
    let _ = req.as_reader().read_to_string(&mut body);
    let body_json: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    let get = |name: &str| -> Option<String> {
        param(&query, name).or_else(|| match &body_json[name] {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    };

    let seg: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(decode).collect();
    let seg: Vec<&str> = seg.iter().map(|s| s.as_str()).collect();

    // Files for Home Assistant (no password needed: nothing secret in them).
    if seg.first() == Some(&"ha") {
        let cfg = b.cfg();
        let ip = crate::server::local_ip();
        let text = match seg.get(1).copied() {
            Some("install.sh") => crate::ha::install_script(&ip, cfg.api_port),
            Some("dashboard.yaml") => crate::ha::dashboard_yaml("chromecast"),
            Some("voice.yaml") => crate::ha::voice_yaml(),
            _ => {
                respond(req, 404, json_reply(false, "unknown address", None));
                return;
            }
        };
        b.log("Home Assistant downloaded the integration files");
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..]).unwrap();
        let _ = req.respond(tiny_http::Response::from_string(text).with_header(header));
        return;
    }

    let token = b.cfg().api_token;
    if !token.trim().is_empty() {
        let header_ok = req.headers().iter().any(|h| {
            h.field.equiv("Authorization") && h.value.as_str().trim() == format!("Bearer {}", token.trim())
        });
        if !header_ok && get("token").as_deref() != Some(token.trim()) {
            respond(req, 401, json_reply(false, "wrong or missing token", None));
            return;
        }
    }

    let num = |name: &str| get(name).and_then(|v| v.trim().parse::<u32>().ok());
    let flag = |name: &str| matches!(get(name).as_deref(), Some("1" | "true" | "yes"));
    let quiet = matches!(seg.as_slice(), ["status"] | ["info"] | ["apps"] | ["keys"] | []);
    let mut extra = None;
    let result: Reply = match seg.as_slice() {
        [] => Ok("TV Launcher Bridge is running. Try /status, /info, /launch/<app>, /key/<button>, /watch?show=<name>".into()),
        ["status"] => {
            // Always answer (so Home Assistant can show "off"), with ok=false when the TV is unreachable.
            let r = b.refresh_status();
            let mut st = b.status_json();
            st["ok"] = serde_json::json!(r.is_ok());
            st["message"] = serde_json::json!(match &r { Ok(m) => m.clone(), Err(e) => e.clone() });
            respond(req, 200, st.to_string());
            return;
        }
        ["info"] | ["apps"] | ["keys"] => {
            let cfg = b.cfg();
            extra = Some(serde_json::json!({
                "name": "Chromecast",
                "version": env!("CARGO_PKG_VERSION"),
                "apps": cfg.apps.iter().map(|a| serde_json::json!({"name": a.name, "package": a.package})).collect::<Vec<_>>(),
                "keys": crate::bridge::KEYS.iter().map(|(id, label, _)| serde_json::json!({"id": id, "label": label})).collect::<Vec<_>>(),
                "raw_commands_allowed": cfg.allow_raw_commands,
            }));
            Ok(format!("{} apps", cfg.apps.len()))
        }
        ["manage", "state"] => {
            let cfg = b.cfg();
            extra = Some(serde_json::json!({"status": b.status_json(), "host": cfg.tv_host, "port": cfg.tv_port, "paired_with": cfg.paired_with}));
            Ok("Current connection state".into())
        }
        ["manage", action] if req.method() == &tiny_http::Method::Post => match *action {
            "discover" => b.find_tvs(),
            "apps" => b.load_apps_from_tv(),
            "connect" => b.test_connection(),
            "pair" | "configure" => {
                let host = get("host").unwrap_or_default();
                let port = num("port").filter(|p| *p > 0 && *p <= 65535);
                if host.parse::<std::net::Ipv4Addr>().is_err() || port.is_none() {
                    Err("Enter a valid TV IPv4 address and connection port (1-65535)".into())
                } else {
                    b.disconnect();
                    {
                        let mut s = b.shared.lock().unwrap();
                        s.cfg.tv_host = host.clone();
                        s.cfg.tv_port = port.unwrap() as u16;
                        s.cfg.save();
                    }
                    if *action == "pair" {
                        match (num("pair_port"), get("code")) {
                            (Some(p), Some(code)) if p > 0 && p <= 65535 && code.len() == 6 && code.bytes().all(|c| c.is_ascii_digit()) => b.pair(&host, p as u16, &code),
                            _ => Err("Enter the pairing port and six-digit pairing code".into()),
                        }
                    } else { Ok("Saved connection settings".into()) }
                }
            }
            _ => Err("Unknown management action".into()),
        },
        ["launch", app] | ["open", app] => b.launch(app),
        ["launch"] | ["open"] => match get("app") {
            Some(a) => b.launch(&a),
            None => Err("say which app, e.g. /launch/jellyfin".into()),
        },
        ["close", app] => b.close_app(app),
        ["close"] => match get("app") {
            Some(a) => b.close_app(&a),
            None => Err("say which app, e.g. /close/youtube".into()),
        },
        ["key", k] => b.key(k, flag("hold")),
        ["power", "on"] | ["wake"] => b.power(true),
        ["power", "off"] | ["sleep"] => b.power(false),
        ["link"] => match get("url") {
            Some(u) => b.open_link(&u, get("app").as_deref()),
            None => Err("add ?url=<link>".into()),
        },
        ["search"] => match get("query").or_else(|| get("q")) {
            Some(q) => b.search(&q, get("app").as_deref()),
            None => Err("add ?query=<words>".into()),
        },
        ["text"] => match get("value").or_else(|| get("text")) {
            Some(t) => b.type_text(&t),
            None => Err("add ?value=<text>".into()),
        },
        ["intent"] => match serde_json::from_value::<crate::bridge::IntentReq>(body_json.clone()) {
            Ok(i) => b.send_intent(&i),
            Err(e) => Err(format!("send the command as JSON: {e}")),
        },
        ["adb"] => match get("command") {
            Some(c) => b.raw(&c).map(|out| {
                extra = Some(serde_json::json!({ "output": out }));
                "Ran the command".to_string()
            }),
            None => Err("send {\"command\": \"...\"}".into()),
        },
        ["jellyfin", "play"] => match get("show") {
            Some(s) => b.jellyfin_play(&s, num("season"), num("episode")),
            None => Err("add ?show=<name>".into()),
        },
        ["watch"] => match get("show") {
            Some(s) => b.watch(&s, num("season"), num("episode")),
            None => Err("add ?show=<name>".into()),
        },
        _ => {
            respond(req, 404, json_reply(false, "unknown address", None));
            return;
        }
    };
    if quiet && result.is_ok() {
        respond(req, 200, json_reply(true, result.as_deref().unwrap_or(""), extra));
        return;
    }
    match &result {
        Ok(m) => b.log(format!("Home Assistant: {m}")),
        Err(e) => b.log(format!("Home Assistant asked for {path}: {e}")),
    }
    match result {
        Ok(m) => respond(req, 200, json_reply(true, &m, extra)),
        Err(e) => respond(req, 500, json_reply(false, &e, None)),
    }
}

fn respond(req: tiny_http::Request, code: u16, body: String) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let _ = req.respond(
        tiny_http::Response::from_string(body)
            .with_status_code(code)
            .with_header(header),
    );
}

/// This computer's address on the home network (best guess).
pub fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.168.0.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "THIS-COMPUTER-IP".into())
}
