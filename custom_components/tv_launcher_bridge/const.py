"""Constants for TV Launcher Bridge."""

DOMAIN = "tv_launcher_bridge"

CONF_TOKEN = "token"

# Filled in by TV Launcher Bridge when it hands out these files.
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8125  # __BRIDGE_PORT__

SCAN_SECONDS = 10

# Icons for the remote buttons, by button id.
KEY_ICONS = {
    "up": "mdi:chevron-up",
    "down": "mdi:chevron-down",
    "left": "mdi:chevron-left",
    "right": "mdi:chevron-right",
    "ok": "mdi:circle-medium",
    "back": "mdi:arrow-left",
    "home": "mdi:home",
    "menu": "mdi:menu",
    "settings": "mdi:cog",
    "search": "mdi:magnify",
    "assistant": "mdi:microphone",
    "input": "mdi:import",
    "guide": "mdi:television-guide",
    "info": "mdi:information-outline",
    "captions": "mdi:closed-caption",
    "play_pause": "mdi:play-pause",
    "play": "mdi:play",
    "pause": "mdi:pause",
    "stop": "mdi:stop",
    "next": "mdi:skip-next",
    "previous": "mdi:skip-previous",
    "rewind": "mdi:rewind",
    "fast_forward": "mdi:fast-forward",
    "volume_up": "mdi:volume-plus",
    "volume_down": "mdi:volume-minus",
    "mute": "mdi:volume-mute",
    "channel_up": "mdi:arrow-up-bold",
    "channel_down": "mdi:arrow-down-bold",
    "power": "mdi:power",
    "wake": "mdi:power-on",
    "sleep": "mdi:power-sleep",
    "delete": "mdi:backspace-outline",
    "enter": "mdi:keyboard-return",
}
