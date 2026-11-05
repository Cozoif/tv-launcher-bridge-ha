# TV Launcher Bridge

Control a Chromecast with Google TV from Home Assistant over wireless ADB.

1. Install and start the bridge from the Home Assistant app store using this repository. Set an API token and use the app web UI to pair the Chromecast.
2. In HACS, add this repository under Custom repositories as an Integration, then download TV Launcher Bridge and restart Home Assistant.
3. In Settings → Devices & services, add TV Launcher Bridge with host `127.0.0.1`, port `8125`, and the app's API token.

See [bridge setup and pairing](tv_launcher_bridge/DOCS.md).
