mod adb;
mod bridge;
mod config;
mod ha;
mod mdns;
mod server;
mod spake2;
mod tls;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use egui::{Color32, CornerRadius, Margin, RichText, Stroke, Vec2};

use bridge::Bridge;
use config::{AppEntry, Config};
use server::ApiServer;

const BG: Color32 = Color32::from_rgb(18, 20, 26);
const PANEL: Color32 = Color32::from_rgb(26, 29, 37);
const CARD: Color32 = Color32::from_rgb(34, 38, 48);
const ACCENT: Color32 = Color32::from_rgb(64, 196, 170);
const MUTED: Color32 = Color32::from_rgb(150, 156, 170);
const GOOD: Color32 = Color32::from_rgb(80, 200, 120);
const BAD: Color32 = Color32::from_rgb(230, 100, 90);
const WAIT: Color32 = Color32::from_rgb(220, 180, 80);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Apps,
    Play,
    HomeAssistant,
    Activity,
}

struct App {
    bridge: Bridge,
    api: Arc<ApiServer>,
    tab: Tab,
    ip: String,
    new_name: String,
    new_package: String,
    show: String,
    season: String,
    episode: String,
    link: String,
    link_app: String,
    text: String,
    search: String,
    search_app: String,
    pair_code: String,
    save_folder: String,
    ha_view: u8,
    // screenshot mode
    shot: Option<PathBuf>,
    frame: u32,
    shot_done: Arc<AtomicBool>,
    shot_requested: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>, tab: Tab) -> Self {
        let cfg = Config::load();
        let bridge = Bridge::new(cfg);
        let ctx = cc.egui_ctx.clone();
        bridge.set_repaint(Box::new(move || ctx.request_repaint()));
        style(&cc.egui_ctx);
        let api = Arc::new(ApiServer::new());
        api.start(&bridge);
        bridge.log("Ready. Type the TV's address and press Connect.");
        Self {
            bridge,
            api,
            tab,
            ip: server::local_ip(),
            new_name: String::new(),
            new_package: String::new(),
            show: String::new(),
            season: String::new(),
            episode: String::new(),
            link: String::new(),
            link_app: String::new(),
            text: String::new(),
            search: String::new(),
            search_app: String::new(),
            pair_code: String::new(),
            save_folder: "~/tv_launcher_bridge-for-home-assistant".into(),
            ha_view: 0,
            shot,
            frame: 0,
            shot_done: Arc::new(AtomicBool::new(false)),
            shot_requested: false,
        }
    }

    fn save(&self) {
        self.bridge.shared.lock().unwrap().cfg.save();
    }
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(14, 16, 21);
    v.selection.bg_fill = ACCENT.linear_multiply(0.6);
    v.hyperlink_color = ACCENT;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(48, 54, 68);
    v.widgets.inactive.bg_fill = Color32::from_rgb(48, 54, 68);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 68, 86);
    v.widgets.hovered.bg_fill = Color32::from_rgb(60, 68, 86);
    v.widgets.active.weak_bg_fill = ACCENT.linear_multiply(0.5);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(6);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
    });
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().size(15.0));
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(10.0);
}

fn pill(ui: &mut egui::Ui, color: Color32, text: &str) {
    egui::Frame::new()
        .fill(color.linear_multiply(0.18))
        .stroke(Stroke::new(1.0, color.linear_multiply(0.7)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, color);
                ui.label(RichText::new(text).color(Color32::from_gray(225)).size(13.0));
            });
        });
}

fn tile_color(package: &str) -> Color32 {
    let mut h: u32 = 2166136261;
    for b in package.bytes() {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    let palette = [
        Color32::from_rgb(88, 101, 242),
        Color32::from_rgb(214, 69, 80),
        Color32::from_rgb(46, 160, 120),
        Color32::from_rgb(232, 140, 48),
        Color32::from_rgb(150, 90, 220),
        Color32::from_rgb(40, 150, 200),
        Color32::from_rgb(200, 90, 160),
        Color32::from_rgb(120, 150, 60),
    ];
    palette[(h as usize) % palette.len()]
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.screenshot_tick(&ctx);

        let (tv, busy, api_status, api_running, port) = {
            let s = self.bridge.shared.lock().unwrap();
            (s.tv.clone(), s.busy, s.api_status.clone(), s.api_running, s.cfg.api_port)
        };

        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::symmetric(16, 10)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, CornerRadius::same(4), ACCENT);
                    p.rect_filled(r.shrink(3.0), CornerRadius::same(2), BG);
                    p.circle_filled(r.center(), 4.0, ACCENT);
                    ui.label(RichText::new("TV Launcher Bridge").size(20.0).strong());
                    ui.label(RichText::new("open TV apps from Home Assistant").color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if busy > 0 {
                            ui.spinner();
                        }
                        let (c, t) = if api_running {
                            (GOOD, format!("Home Assistant link: {}:{port}", self.ip))
                        } else {
                            (BAD, "Home Assistant link: off (see left)".to_string())
                        };
                        pill(ui, c, &t);
                        let (c, t) = match tv.connected {
                            Some(true) => (
                                GOOD,
                                if tv.current_app.is_empty() {
                                    "TV connected".to_string()
                                } else {
                                    format!("TV: {}", tv.current_app)
                                },
                            ),
                            Some(false) => (BAD, "TV not reachable".into()),
                            None => (WAIT, "TV not connected yet".into()),
                        };
                        pill(ui, c, &t);
                    });
                });
            });

        egui::Panel::left("settings")
            .exact_size(310.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.settings_panel(ui, &tv, &api_status));
            });

        egui::Panel::right("remote")
            .exact_size(230.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(12)))
            .show(ui, |ui| self.remote_panel(ui, &tv));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (t, label) in [
                        (Tab::Apps, "Apps"),
                        (Tab::Play, "Play something"),
                        (Tab::HomeAssistant, "Home Assistant setup"),
                        (Tab::Activity, "Activity"),
                    ] {
                        let sel = self.tab == t;
                        let txt = RichText::new(label).size(15.0).color(if sel { Color32::WHITE } else { MUTED });
                        let b = egui::Button::new(txt)
                            .fill(if sel { ACCENT.linear_multiply(0.35) } else { Color32::TRANSPARENT })
                            .corner_radius(CornerRadius::same(8));
                        if ui.add(b).clicked() {
                            self.tab = t;
                        }
                    }
                });
                ui.add_space(8.0);
                match self.tab {
                    Tab::Apps => self.apps_tab(ui),
                    Tab::Play => self.play_tab(ui),
                    Tab::HomeAssistant => self.ha_tab(ui),
                    Tab::Activity => self.activity_tab(ui),
                }
            });
    }
}

impl App {
    fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frame += 1;
        ctx.request_repaint();
        if self.shot_done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frame >= 120 && !self.shot_requested {
            self.shot_requested = true;
            let done = self.shot_done.clone();
            let c2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    buf.extend_from_slice(&p.to_srgba_unmultiplied());
                }
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    Some(im) => {
                        if let Err(e) = im.save(&path) {
                            eprintln!("could not save screenshot: {e}");
                        }
                    }
                    None => eprintln!("could not build screenshot image"),
                }
                done.store(true, Ordering::SeqCst);
                c2.request_repaint();
            });
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui, tv: &bridge::TvState, api_status: &str) {
        let mut changed = false;
        let mut reconnect = false;
        let mut restart_api = false;
        section(ui, "Chromecast with Google TV", |ui| {
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Find TV").strong()).on_hover_text("Looks for TVs with Wireless debugging on this network").clicked() {
                    self.bridge.spawn(|b| b.find_tvs());
                }
                ui.label(RichText::new("needs Wireless debugging on").size(12.5).color(MUTED));
            });
            let found = self.bridge.shared.lock().unwrap().found.clone();
            for f in found {
                let what = if f.kind == mdns::Kind::Pairing { "pairing" } else { "connect" };
                let label = format!("{}  {}:{}  ({what})", f.name, f.ip, f.port);
                if ui.small_button(RichText::new(label).size(12.0)).on_hover_text("Use this").clicked() {
                    let mut s = self.bridge.shared.lock().unwrap();
                    s.cfg.tv_host = f.ip.clone();
                    if f.kind == mdns::Kind::Pairing {
                        s.cfg.pair_port = f.port;
                    } else {
                        s.cfg.tv_port = f.port;
                    }
                    changed = true;
                    reconnect = true;
                }
            }
            let mut s = self.bridge.shared.lock().unwrap();
            ui.label(RichText::new("TV address").color(MUTED));
            let r = ui.add(egui::TextEdit::singleline(&mut s.cfg.tv_host).hint_text("e.g. 192.168.1.50").desired_width(f32::INFINITY));
            if r.changed() {
                changed = true;
                reconnect = true;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Connect port").color(MUTED))
                    .on_hover_text("Wireless debugging › \"IP address & Port\" (the number after the colon)");
                if ui.add(egui::DragValue::new(&mut s.cfg.tv_port).range(1..=65535)).changed() {
                    changed = true;
                    reconnect = true;
                }
            });
            changed |= ui
                .checkbox(&mut s.cfg.auto_find_port, RichText::new("Find the new port by itself when it changes").size(12.5))
                .changed();
            let paired = s.cfg.paired_with.clone();
            drop(s);

            ui.add_space(4.0);
            egui::Frame::new()
                .fill(Color32::from_rgb(28, 32, 41))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let title = if paired.is_empty() { "Pair (first time only)".to_string() } else { format!("Paired with {paired}") };
                    ui.label(RichText::new(title).strong().size(13.0).color(if paired.is_empty() { WAIT } else { GOOD }));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Code").color(MUTED));
                        ui.add(egui::TextEdit::singleline(&mut self.pair_code).hint_text("6 digits").desired_width(70.0));
                        ui.label(RichText::new("Port").color(MUTED));
                        let mut s = self.bridge.shared.lock().unwrap();
                        changed |= ui.add(egui::DragValue::new(&mut s.cfg.pair_port).range(0..=65535)).changed();
                    });
                    let (host, pport) = {
                        let s = self.bridge.shared.lock().unwrap();
                        (s.cfg.tv_host.clone(), s.cfg.pair_port)
                    };
                    let ready = !host.trim().is_empty() && pport > 0 && self.pair_code.trim().len() >= 6;
                    if ui.add_enabled(ready, egui::Button::new("Pair with TV")).clicked() {
                        let code = self.pair_code.trim().to_string();
                        self.bridge.spawn(move |b| b.pair(&host, pport, &code));
                        self.pair_code.clear();
                    }
                });

            ui.horizontal(|ui| {
                if ui.button(RichText::new("Connect").strong()).clicked() {
                    self.save();
                    self.bridge.disconnect();
                    self.bridge.spawn(|b| b.test_connection());
                }
                if ui.button("Load apps from TV").clicked() {
                    self.bridge.spawn(|b| b.load_apps_from_tv());
                }
            });
            if !tv.message.is_empty() {
                let c = if tv.connected == Some(true) { GOOD } else { BAD };
                ui.label(RichText::new(&tv.message).color(c).size(12.5));
            }
            ui.collapsing("How to set up wireless debugging", |ui| {
                ui.label(RichText::new(
                    "1. On the TV: Settings › System › About › tap \"Android TV OS build\" 7 times.\n\
                     2. Settings › System › Developer options › turn on Wireless debugging.\n\
                     3. Open Wireless debugging › \"Pair device with pairing code\". Press Find TV here (or type the address and the \
                     port shown under the code), type the code and press Pair with TV.\n\
                     4. Back on the Wireless debugging screen, \"IP address & Port\" shows the connect port. Press Find TV or type it, \
                     then Connect.\n\n\
                     The connect port changes when the TV restarts; with \"Find the new port by itself\" on, this program follows it. \
                     Plain network debugging on port 5555 also works.",
                ).size(12.5).color(MUTED));
            });
        });

        section(ui, "Home Assistant link", |ui| {
            let mut s = self.bridge.shared.lock().unwrap();
            ui.label(RichText::new("Home Assistant calls this computer at:").color(MUTED));
            ui.label(RichText::new(ha::base_url(&self.ip, s.cfg.api_port)).monospace().color(ACCENT));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Port").color(MUTED));
                changed |= ui.add(egui::DragValue::new(&mut s.cfg.api_port).range(1024..=65535)).changed();
                if ui.button("Restart").clicked() {
                    restart_api = true;
                }
            });
            ui.label(RichText::new("Password (optional)").color(MUTED));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.cfg.api_token).hint_text("leave empty for none").password(true).desired_width(f32::INFINITY))
                .changed();
            changed |= ui
                .checkbox(&mut s.cfg.allow_raw_commands, RichText::new("Allow any ADB command from Home Assistant").size(12.5))
                .on_hover_text("Lets Home Assistant run any command on the TV (the \"ADB command\" action). Off is safer.")
                .changed();
            ui.label(RichText::new(api_status).size(12.5).color(MUTED));
        });

        section(ui, "Jellyfin (to play shows)", |ui| {
            let mut s = self.bridge.shared.lock().unwrap();
            ui.label(RichText::new("Server address").color(MUTED));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.cfg.jellyfin_url).hint_text("http://192.168.1.20:8096").desired_width(f32::INFINITY))
                .changed();
            ui.label(RichText::new("API key").color(MUTED))
                .on_hover_text("Jellyfin dashboard › API Keys › +");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.cfg.jellyfin_api_key).password(true).desired_width(f32::INFINITY))
                .changed();
            ui.label(RichText::new("TV name in Jellyfin (optional)").color(MUTED));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.cfg.jellyfin_device).hint_text("picked automatically").desired_width(f32::INFINITY))
                .changed();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Wait after opening").color(MUTED));
                changed |= ui.add(egui::DragValue::new(&mut s.cfg.jellyfin_wait_s).range(1..=30).suffix(" s")).changed();
            });
            drop(s);
            if ui.button("Test Jellyfin").clicked() {
                self.save();
                self.bridge.spawn(|b| b.jellyfin_test());
            }
        });

        if changed {
            self.save();
        }
        if reconnect {
            self.bridge.disconnect();
        }
        if restart_api {
            self.save();
            self.api.start(&self.bridge);
        }
    }

    fn remote_panel(&mut self, ui: &mut egui::Ui, tv: &bridge::TvState) {
        ui.label(RichText::new("Remote").strong().size(15.0));
        ui.add_space(6.0);
        let b = self.bridge.clone();
        let press = |k: &'static str| {
            let b = b.clone();
            move || b.spawn(move |b| b.key(k, false))
        };
        ui.horizontal(|ui| {
            if ui.add_sized([96.0, 30.0], egui::Button::new(RichText::new("Turn on").color(GOOD))).clicked() {
                self.bridge.spawn(|b| b.power(true));
            }
            if ui.add_sized([96.0, 30.0], egui::Button::new(RichText::new("Sleep").color(MUTED))).clicked() {
                self.bridge.spawn(|b| b.power(false));
            }
        });
        ui.add_space(10.0);
        // D-pad, painted by hand so it doesn't disturb the layout.
        let size = 196.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size), egui::Sense::hover());
        let c = rect.center();
        ui.painter().circle_filled(c, size / 2.0, CARD);
        ui.painter().circle_stroke(c, size / 2.0, Stroke::new(1.0, Color32::from_gray(60)));
        let d = 64.0;
        for (k, dir) in [
            ("UP", Vec2::new(0.0, -1.0)),
            ("DOWN", Vec2::new(0.0, 1.0)),
            ("LEFT", Vec2::new(-1.0, 0.0)),
            ("RIGHT", Vec2::new(1.0, 0.0)),
        ] {
            let center = c + dir * d;
            let r = egui::Rect::from_center_size(center, Vec2::splat(52.0));
            let resp = ui.interact(r, ui.scope_id().with(k), egui::Sense::click());
            if resp.hovered() {
                ui.painter().circle_filled(center, 24.0, Color32::from_rgb(48, 54, 68));
            }
            let side = Vec2::new(-dir.y, dir.x);
            let tip = center + dir * 9.0;
            let base = center - dir * 6.0;
            ui.painter().add(egui::Shape::convex_polygon(
                vec![tip, base + side * 9.0, base - side * 9.0],
                if resp.hovered() { ACCENT } else { Color32::from_gray(210) },
                Stroke::NONE,
            ));
            if resp.clicked() {
                press(k)();
            }
        }
        let okr = egui::Rect::from_center_size(c, Vec2::splat(64.0));
        let ok = ui.interact(okr, ui.scope_id().with("OK"), egui::Sense::click());
        ui.painter().circle_filled(c, 32.0, if ok.hovered() { ACCENT.linear_multiply(0.7) } else { ACCENT.linear_multiply(0.45) });
        ui.painter().text(c, egui::Align2::CENTER_CENTER, "OK", egui::FontId::proportional(17.0), Color32::WHITE);
        if ok.clicked() {
            press("OK")();
        }
        ui.add_space(10.0);
        let grid = [
            [("Back", "BACK"), ("Home", "HOME")],
            [("Play / Pause", "PLAY_PAUSE"), ("Stop", "STOP")],
            [("Rewind", "REWIND"), ("Forward", "FORWARD")],
            [("Volume -", "VOLUME_DOWN"), ("Volume +", "VOLUME_UP")],
            [("Mute", "MUTE"), ("Menu", "MENU")],
            [("Search", "SEARCH"), ("Settings", "SETTINGS")],
        ];
        for row in grid {
            ui.horizontal(|ui| {
                for (label, k) in row {
                    if ui.add_sized([98.0, 28.0], egui::Button::new(label)).clicked() {
                        press(k)();
                    }
                }
            });
        }
        ui.add_space(12.0);
        ui.separator();
        ui.label(RichText::new("Now on the TV").color(MUTED));
        let now = match (tv.connected, tv.awake) {
            (Some(true), Some(false)) => "Asleep".to_string(),
            (Some(true), _) if !tv.current_app.is_empty() => tv.current_app.clone(),
            (Some(true), _) => "Home screen".to_string(),
            _ => "—".to_string(),
        };
        ui.label(RichText::new(now).size(16.0).strong());
        if ui.small_button("Refresh").clicked() {
            self.bridge.spawn(|b| b.refresh_status());
        }
    }

    fn apps_tab(&mut self, ui: &mut egui::Ui) {
        let apps = self.bridge.cfg().apps;
        ui.label(
            RichText::new("Click an app to open it on the TV. Home Assistant can open the same apps by name.")
                .color(MUTED),
        );
        ui.add_space(6.0);
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 90.0)
            .show(ui, |ui| {
                let tile = Vec2::new(132.0, 112.0);
                let cols = ((ui.available_width() + 10.0) / (tile.x + 10.0)).floor().max(1.0) as usize;
                for (row_i, row) in apps.chunks(cols).enumerate() {
                    ui.horizontal(|ui| {
                        for (j, app) in row.iter().enumerate() {
                            let idx = row_i * cols + j;
                            let (rect, resp) = ui.allocate_exact_size(tile, egui::Sense::click());
                            let hovered = resp.hovered();
                            let p = ui.painter();
                            p.rect_filled(rect, CornerRadius::same(12), if hovered { Color32::from_rgb(44, 50, 62) } else { CARD });
                            if hovered {
                                p.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                            }
                            let icon = egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 42.0), Vec2::splat(52.0));
                            let col = tile_color(&app.package);
                            p.rect_filled(icon, CornerRadius::same(12), col);
                            p.rect_filled(
                                egui::Rect::from_min_max(icon.left_center(), icon.max),
                                CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 },
                                Color32::from_black_alpha(40),
                            );
                            let letter: String = app.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                            p.text(icon.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(26.0), Color32::WHITE);
                            p.text(
                                rect.center_bottom() - Vec2::new(0.0, 20.0),
                                egui::Align2::CENTER_CENTER,
                                &app.name,
                                egui::FontId::proportional(14.0),
                                Color32::from_gray(230),
                            );
                            let resp = resp.on_hover_text(format!("{}\nRight-click to remove", app.package));
                            if resp.clicked() {
                                let pkg = app.package.clone();
                                self.bridge.spawn(move |b| b.launch(&pkg));
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Remove from list").clicked() {
                                    remove = Some(idx);
                                    ui.close();
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        if let Some(i) = remove {
            let mut s = self.bridge.shared.lock().unwrap();
            if i < s.cfg.apps.len() {
                s.cfg.apps.remove(i);
                s.cfg.save();
            }
        }
        ui.add_space(8.0);
        section(ui, "Add an app", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Name, e.g. Jellyfin").desired_width(170.0));
                ui.add(egui::TextEdit::singleline(&mut self.new_package).hint_text("Package, e.g. org.jellyfin.androidtv").desired_width(260.0));
                let ok = adb::is_package_name(self.new_package.trim());
                if ui.add_enabled(ok, egui::Button::new("Add")).clicked() {
                    let pkg = self.new_package.trim().to_string();
                    let name = if self.new_name.trim().is_empty() { config::friendly_name(&pkg) } else { self.new_name.trim().to_string() };
                    let mut s = self.bridge.shared.lock().unwrap();
                    s.cfg.apps.retain(|a| a.package != pkg);
                    s.cfg.apps.push(AppEntry { name, package: pkg });
                    s.cfg.save();
                    self.new_name.clear();
                    self.new_package.clear();
                }
            });
        });
    }

    fn play_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            section(ui, "Watch a show or film on Jellyfin", |ui| {
                ui.label(RichText::new("Wakes the TV, opens Jellyfin and plays it. For a series it picks the next unwatched episode, unless you give a season and episode.").color(MUTED));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.show).hint_text("Show or film name").desired_width(260.0));
                    ui.label(RichText::new("Season").color(MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.season).desired_width(36.0));
                    ui.label(RichText::new("Episode").color(MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.episode).desired_width(36.0));
                });
                ui.horizontal(|ui| {
                    let s = self.show.trim().to_string();
                    let season = self.season.trim().parse().ok();
                    let ep = self.episode.trim().parse().ok();
                    if ui.add_enabled(!s.is_empty(), egui::Button::new(RichText::new("Turn on, open Jellyfin and play").strong())).clicked() {
                        let s = s.clone();
                        self.bridge.spawn(move |b| b.watch(&s, season, ep));
                    }
                    if ui.add_enabled(!s.is_empty(), egui::Button::new("Play now (Jellyfin already open)")).clicked() {
                        self.bridge.spawn(move |b| b.jellyfin_play(&s, season, ep));
                    }
                });
            });
            section(ui, "Open a link in an app", |ui| {
                ui.label(RichText::new("Works with apps that accept links, e.g. a YouTube video address. Leave the app empty to let the TV choose.").color(MUTED));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.link).hint_text("https://www.youtube.com/watch?v=…").desired_width(320.0));
                    ui.add(egui::TextEdit::singleline(&mut self.link_app).hint_text("App (optional)").desired_width(120.0));
                    if ui.add_enabled(!self.link.trim().is_empty(), egui::Button::new("Open")).clicked() {
                        let (l, a) = (self.link.clone(), self.link_app.clone());
                        self.bridge.spawn(move |b| b.open_link(&l, Some(&a)));
                    }
                });
            });
            section(ui, "Search", |ui| {
                ui.label(RichText::new("Searches the whole TV, or inside one app if you name it.").color(MUTED));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("What to look for").desired_width(320.0));
                    ui.add(egui::TextEdit::singleline(&mut self.search_app).hint_text("App (optional)").desired_width(120.0));
                    if ui.add_enabled(!self.search.trim().is_empty(), egui::Button::new("Search")).clicked() {
                        let (q, a) = (self.search.clone(), self.search_app.clone());
                        self.bridge.spawn(move |b| b.search(&q, Some(&a)));
                    }
                });
            });
            section(ui, "Type on the TV", |ui| {
                ui.label(RichText::new("Types into whatever search box is open on the TV, in any app.").color(MUTED));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.text).hint_text("Text to type").desired_width(320.0));
                    if ui.add_enabled(!self.text.is_empty(), egui::Button::new("Type")).clicked() {
                        let t = self.text.clone();
                        self.bridge.spawn(move |b| b.type_text(&t));
                    }
                });
            });
        });
    }

    fn ha_tab(&mut self, ui: &mut egui::Ui) {
        let cfg = self.bridge.cfg();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.ha_view, 0, "Install the integration");
            ui.selectable_value(&mut self.ha_view, 1, "Remote card");
            ui.selectable_value(&mut self.ha_view, 2, "Voice assistant");
            ui.selectable_value(&mut self.ha_view, 3, "Web addresses");
        });
        ui.add_space(6.0);
        if self.ha_view == 0 {
            egui::ScrollArea::vertical().show(ui, |ui| {
                section(ui, "What you get in Home Assistant", |ui| {
                    ui.label(RichText::new(
                        "A \"Chromecast\" device with: a media player (on/off, play/pause, next, volume, what's playing, \
                         apps as sources), a remote (any button, hold, repeat), a button for every remote key, \
                         an app picker, text boxes to play a Jellyfin show / search / type / open a link, \
                         and actions for automations and voice: open app, close app, press button, play show, \
                         open link, search, type text, send a command to an app, and (if allowed) any ADB command. \
                         No MQTT needed.",
                    ).color(MUTED));
                });
                section(ui, "Option A: one command in Home Assistant's terminal", |ui| {
                    ui.label(RichText::new(
                        "Open the Terminal (the \"Terminal & SSH\" add-on), paste this and press Enter. \
                         This program must be running.",
                    ).color(MUTED));
                    let cmd = ha::install_command(&self.ip, cfg.api_port);
                    ui.horizontal(|ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(14, 16, 21))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.label(RichText::new(&cmd).monospace().color(ACCENT));
                            });
                        if ui.button("Copy").clicked() {
                            ui.ctx().copy_text(cmd.clone());
                            self.bridge.log("Copied the install command");
                        }
                    });
                });
                section(ui, "Option B: save the files and copy them yourself", |ui| {
                    ui.label(RichText::new(
                        "Saves a custom_components folder. Copy it into Home Assistant's config folder \
                         (for example with the Samba share or File editor add-on).",
                    ).color(MUTED));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.save_folder).desired_width(340.0));
                        if ui.button("Save files").clicked() {
                            let folder = self.save_folder.clone();
                            let ip = self.ip.clone();
                            let port = cfg.api_port;
                            self.bridge.spawn(move |_| ha::save_integration(&folder, &ip, port));
                        }
                    });
                });
                section(ui, "Then", |ui| {
                    ui.label(RichText::new(
                        "1. Restart Home Assistant.\n\
                         2. Settings › Devices & services › Add integration › TV Launcher Bridge.\n\
                         3. The address is filled in for you; press Submit.\n\
                         4. Optional: add the Remote card (next tab) to a dashboard.",
                    ).color(Color32::from_gray(215)));
                    ui.label(RichText::new(format!("This computer: {}", ha::base_url(&self.ip, cfg.api_port))).monospace().color(ACCENT));
                });
            });
            return;
        }
        let (intro, text) = match self.ha_view {
            1 => (
                "A dashboard card laid out like a remote, with what's playing, every button (hold for a long press), the app picker and the text boxes. Dashboard › Edit › Add card › Manual, then paste.",
                ha::dashboard_yaml("chromecast"),
            ),
            2 => (
                "\"Turn on the TV\", \"pause the TV\" and volume already work through the media player. These add \"play Bluey on the TV\", \"open YouTube on the TV\" and search, plus scripts an AI assistant can use.",
                ha::voice_yaml(),
            ),
            _ => (
                "The same commands as plain web addresses, for anything that isn't Home Assistant.",
                ha::api_help(&self.ip, cfg.api_port),
            ),
        };
        ui.label(RichText::new(intro).color(MUTED));
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(text.clone());
            self.bridge.log("Copied to the clipboard");
        }
        egui::Frame::new()
            .fill(Color32::from_rgb(14, 16, 21))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut text.as_str()).code_editor().desired_width(f32::INFINITY));
                });
            });
    }

    fn activity_tab(&mut self, ui: &mut egui::Ui) {
        let log: Vec<String> = self.bridge.shared.lock().unwrap().log.iter().cloned().collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for l in log {
                let c = if l.contains("Problem") || l.contains(": can't") { BAD } else { Color32::from_gray(210) };
                ui.label(RichText::new(l).monospace().size(12.5).color(c));
            }
        });
    }
}

fn run_service() {
    let bridge = Bridge::new(Config::load());
    let api = ApiServer::new();
    api.start(&bridge);
    println!("Running without a window. Press Ctrl+C to stop.");
    loop {
        std::thread::park();
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--service") {
        run_service();
        return Ok(());
    }
    let shot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    // --tab apps|play|ha|activity: which page to open on.
    let tab = match args.iter().position(|a| a == "--tab").and_then(|i| args.get(i + 1)).map(|s| s.as_str()) {
        Some("play") => Tab::Play,
        Some("ha") => Tab::HomeAssistant,
        Some("activity") => Tab::Activity,
        _ => Tab::Apps,
    };
    if shot.is_some() && std::env::var_os("DISPLAY").is_some() {
        // Wayland stops drawing hidden windows (e.g. a locked screen), which would stall
        // the screenshot; X11 keeps drawing. Done before any other thread starts.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TV Launcher Bridge")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([980.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "TV Launcher Bridge",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shot, tab)))),
    )
}
