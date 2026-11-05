//! The Home Assistant integration files (bundled into this program), an installer
//! script Home Assistant's terminal can run, a ready-made dashboard and voice setup.

use base64::Engine;

macro_rules! file {
    ($p:literal) => {
        ($p, include_str!(concat!("../ha_integration/custom_components/tv_launcher_bridge/", $p)))
    };
}

/// (path inside custom_components/tv_launcher_bridge, contents)
const FILES: &[(&str, &str)] = &[
    file!("__init__.py"),
    file!("api.py"),
    file!("button.py"),
    file!("config_flow.py"),
    file!("const.py"),
    file!("entity.py"),
    file!("manifest.json"),
    file!("media_player.py"),
    file!("remote.py"),
    file!("select.py"),
    file!("sensor.py"),
    file!("services.yaml"),
    file!("strings.json"),
    file!("text.py"),
    file!("translations/en.json"),
];

pub fn base_url(ip: &str, port: u16) -> String {
    format!("http://{ip}:{port}")
}

/// The integration files, with this bridge's address filled in as the default.
pub fn integration_files(ip: &str, port: u16) -> Vec<(String, String)> {
    FILES
        .iter()
        .map(|(p, c)| {
            let c = c
                .replace("__BRIDGE_HOST__", ip)
                .replace("8125  # __BRIDGE_PORT__", &port.to_string());
            (p.to_string(), c)
        })
        .collect()
}

/// Write the integration into `<folder>/custom_components/tv_launcher_bridge`.
pub fn save_integration(folder: &str, ip: &str, port: u16) -> Result<String, String> {
    let folder = folder.trim();
    let base = if let Some(rest) = folder.strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest)
    } else {
        std::path::PathBuf::from(folder)
    };
    let dir = base.join("custom_components").join("tv_launcher_bridge");
    for (p, c) in integration_files(ip, port) {
        let path = dir.join(&p);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("can't create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, c).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(format!("Saved the integration to {}", dir.display()))
}

/// A shell script that installs the integration into Home Assistant's config folder.
pub fn install_script(ip: &str, port: u16) -> String {
    let mut s = String::from(
        "#!/bin/sh\n\
         # TV Launcher Bridge: installs the Home Assistant integration.\n\
         set -e\n\
         CFG=\"$1\"\n\
         if [ -z \"$CFG\" ]; then\n\
         \x20 for d in /config /homeassistant /usr/share/hassio/homeassistant \"$HOME/.homeassistant\"; do\n\
         \x20   if [ -f \"$d/configuration.yaml\" ]; then CFG=\"$d\"; break; fi\n\
         \x20 done\n\
         fi\n\
         if [ -z \"$CFG\" ]; then echo \"Can't find Home Assistant's config folder. Run again with it, e.g.: sh install.sh /config\"; exit 1; fi\n\
         D=\"$CFG/custom_components/tv_launcher_bridge\"\n\
         mkdir -p \"$D/translations\"\n",
    );
    for (p, c) in integration_files(ip, port) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(c.as_bytes());
        s.push_str(&format!("printf '%s' '{b64}' | base64 -d > \"$D/{p}\"\n"));
    }
    s.push_str(
        "echo \"Installed into $D\"\n\
         echo \"Now restart Home Assistant, then: Settings > Devices & services > Add integration > TV Launcher Bridge\"\n",
    );
    s
}

pub fn install_command(ip: &str, port: u16) -> String {
    format!("curl -fsSL {}/ha/install.sh | sh", base_url(ip, port))
}

/// A dashboard card with every control, laid out like a remote.
pub fn dashboard_yaml(entity: &str) -> String {
    let btn = |icon: &str, name: &str, cmd: &str| {
        format!(
            "      - type: button\n        icon: {icon}\n        name: {name}\n        tap_action:\n          action: perform-action\n          perform_action: remote.send_command\n          target:\n            entity_id: remote.{entity}\n          data:\n            command: {cmd}\n        hold_action:\n          action: perform-action\n          perform_action: remote.send_command\n          target:\n            entity_id: remote.{entity}\n          data:\n            command: {cmd}\n            hold_secs: 1\n"
        )
    };
    let blank = "      - type: markdown\n        content: \" \"\n".to_string();
    let grid = |cols: u8, items: Vec<String>| {
        format!("  - type: grid\n    columns: {cols}\n    square: false\n    cards:\n{}", items.concat())
    };
    let mut y = format!(
        "# Dashboard > Edit > Add card > Manual, then paste this.\n\
         # If you renamed the TV in Home Assistant, change \"{entity}\" below to match.\n\
         type: vertical-stack\n\
         cards:\n\
         \x20 - type: media-control\n\
         \x20   entity: media_player.{entity}\n"
    );
    y.push_str(&grid(
        3,
        vec![
            btn("mdi:power", "Power", "power"),
            btn("mdi:home", "Home", "home"),
            btn("mdi:arrow-left", "Back", "back"),
        ],
    ));
    y.push_str(&grid(
        3,
        vec![
            blank.clone(),
            btn("mdi:chevron-up", "Up", "up"),
            blank.clone(),
            btn("mdi:chevron-left", "Left", "left"),
            btn("mdi:circle-medium", "OK", "ok"),
            btn("mdi:chevron-right", "Right", "right"),
            blank.clone(),
            btn("mdi:chevron-down", "Down", "down"),
            blank,
        ],
    ));
    y.push_str(&grid(
        4,
        vec![
            btn("mdi:rewind", "Rewind", "rewind"),
            btn("mdi:play-pause", "Play", "play_pause"),
            btn("mdi:fast-forward", "Forward", "fast_forward"),
            btn("mdi:stop", "Stop", "stop"),
            btn("mdi:skip-previous", "Previous", "previous"),
            btn("mdi:skip-next", "Next", "next"),
            btn("mdi:closed-caption", "Captions", "captions"),
            btn("mdi:information-outline", "Info", "info"),
            btn("mdi:volume-minus", "Vol -", "volume_down"),
            btn("mdi:volume-mute", "Mute", "mute"),
            btn("mdi:volume-plus", "Vol +", "volume_up"),
            btn("mdi:menu", "Menu", "menu"),
            btn("mdi:magnify", "Search", "search"),
            btn("mdi:microphone", "Assistant", "assistant"),
            btn("mdi:cog", "Settings", "settings"),
            btn("mdi:import", "Input", "input"),
        ],
    ));
    y.push_str(&format!(
        "  - type: entities\n    entities:\n\
         \x20     - entity: select.{entity}_app\n\
         \x20     - entity: text.{entity}_play_show_on_jellyfin\n\
         \x20     - entity: text.{entity}_search_on_tv\n\
         \x20     - entity: text.{entity}_type_on_tv\n\
         \x20     - entity: text.{entity}_open_link\n\
         \x20     - entity: sensor.{entity}_now_playing\n"
    ));
    y
}

/// Voice: sentences for the built-in assistant, and scripts any assistant can use.
pub fn voice_yaml() -> String {
    r#"# 1) Save as config/custom_sentences/en/tv.yaml
language: "en"
intents:
  TvWatchShow:
    data:
      - sentences:
          - "(play|put on|watch|start) {show} on [the] (tv|jellyfin)"
  TvOpenApp:
    data:
      - sentences:
          - "(open|launch|start) {app} on [the] tv"
  TvSearch:
    data:
      - sentences:
          - "search [for] {query} on [the] tv"
lists:
  show:
    wildcard: true
  app:
    wildcard: true
  query:
    wildcard: true

# 2) Add to configuration.yaml (then restart)
intent_script:
  TvWatchShow:
    action:
      - action: tv_launcher_bridge.play_show
        data:
          show: "{{ show }}"
    speech:
      text: "Putting on {{ show }}"
  TvOpenApp:
    action:
      - action: media_player.turn_on
        target:
          entity_id: media_player.chromecast
      - action: tv_launcher_bridge.launch_app
        data:
          app: "{{ app }}"
    speech:
      text: "Opening {{ app }}"
  TvSearch:
    action:
      - action: tv_launcher_bridge.search
        data:
          query: "{{ query }}"
    speech:
      text: "Searching for {{ query }}"

# 3) For AI assistants: add these scripts, then expose them
#    (Settings > Voice assistants > Expose). "Turn on the TV", "pause",
#    "volume up" already work through the TV's media player.
script:
  tv_watch_show:
    alias: "Watch a show on the TV"
    description: "Turns on the TV, opens Jellyfin and plays the named show or film."
    fields:
      show:
        description: "Name of the show or film in Jellyfin"
        required: true
        selector: { text: {} }
    sequence:
      - action: tv_launcher_bridge.play_show
        data:
          show: "{{ show }}"
  tv_open_app:
    alias: "Open an app on the TV"
    description: "Turns on the TV and opens any app by name (Jellyfin, YouTube, Netflix...)."
    fields:
      app:
        description: "App name"
        required: true
        selector: { text: {} }
    sequence:
      - action: media_player.turn_on
        target:
          entity_id: media_player.chromecast
      - action: tv_launcher_bridge.launch_app
        data:
          app: "{{ app }}"
  tv_press_button:
    alias: "Press a button on the TV remote"
    description: "Presses a remote button: up, down, left, right, ok, back, home, play_pause, volume_up, volume_down, mute, next, previous."
    fields:
      button:
        description: "Button name"
        required: true
        selector: { text: {} }
    sequence:
      - action: remote.send_command
        target:
          entity_id: remote.chromecast
        data:
          command: "{{ button }}"
"#
    .to_string()
}

pub fn api_help(ip: &str, port: u16) -> String {
    let base = base_url(ip, port);
    format!(
        "{base}/status                     what the TV is doing (JSON)\n\
         {base}/info                       app list and every button\n\
         {base}/launch/jellyfin            open any app by name or package\n\
         {base}/close/youtube              close an app\n\
         {base}/key/play_pause             press a button (add ?hold=1 to long-press)\n\
         {base}/power/on   /power/off\n\
         {base}/watch?show=Bluey           wake TV, open Jellyfin, play next episode\n\
         {base}/watch?show=Bluey&season=2&episode=5\n\
         {base}/search?query=news&app=youtube\n\
         {base}/link?url=<link>&app=youtube\n\
         {base}/text?value=hello           type into the open text box\n\
         {base}/intent   (POST JSON: action, data, package, component, category, mime_type, extras)\n\
         {base}/adb      (POST JSON: command) only if allowed in settings\n\
         {base}/ha/install.sh              the Home Assistant integration installer"
    )
}
