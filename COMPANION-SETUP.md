# TV Bridge companion prototype 0.2.0 / HA app 0.4.0

This is a device-test build. Background app launch, standby reconnection and restart recovery must be verified on the actual Chromecast.

1. Install tv-bridge-companion.apk on the Chromecast and open TV Bridge.
2. Open Remote app launcher setup. The receiver secret is generated on the TV. Copy it privately into the Home Assistant app configuration. Do not send it in chat.
3. Enable TV Bridge under Android Settings > Accessibility. Android may also require Allow restricted settings on the app details screen for a sideloaded APK. Inspect the permission description first.
4. Leave the default allowed app org.jellyfin.androidtv for the first test. Comma-separated Android package names can be added later.
5. Update the HA app to 0.4.0. Set companion_url to http://CHROMECAST_LAN_IP:8130 and companion_token to the receiver secret. Keep api_bind at 127.0.0.1 and allow_raw_commands false. Save and restart the HA app.
6. With the TV awake, run tv_launcher_bridge.launch_app with app: org.jellyfin.androidtv. The result succeeds only when the companion observes Jellyfin in the foreground.
7. Disable wireless debugging. Repeat from the launcher, another foreground app, after standby and after a Chromecast reboot. A force-stopped or disabled accessibility service must be re-enabled manually.
8. Power-on remains the responsibility of the existing Android TV Remote / LG routine in HA. The companion does not wake a disconnected or unpowered Chromecast.

The current HA script has an 18-second wake delay and a 5-second app initialization delay. Those can be tuned after measuring this device. No new HA integration service is required for launching: launch_app and play_sitcom retain their existing names.

The service observes window package changes only, with canRetrieveWindowContent=false. It does not read screen text, execute shell commands or accept arbitrary intents. Launching is limited to the configured package list. Requests use HMAC-SHA256 signatures with timestamps and a replay cache, with a separate random 192-bit companion secret. Keep both device clocks synchronized. HTTP metadata is visible on the LAN; commands are authenticated, not encrypted. Do not forward port 8130 to the internet.

The APK uses the existing local debug signing identity for this personal prototype. Keep that key private. The source archive excludes signing keys and build output.

To return to ADB launch, clear companion_url and companion_token and restart the HA app. Disable the TV Bridge accessibility service to close the companion listener.
