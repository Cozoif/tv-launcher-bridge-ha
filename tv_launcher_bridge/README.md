# TV Launcher Bridge

Headless amd64 HA OS add-on reusing the TV Launcher Bridge Rust ADB engine.
Open DOCS.md for local installation, pairing, migration and verification steps.
Configure an API token of at least 24 characters before starting. Use Open Web UI
for wireless pairing. No PC or APK is needed. Persistent identity and settings
live in /data/tv-launcher-bridge. The default API is loopback-only on port 8125;
Ingress is on 8099 with a Supervisor peer restriction. Host networking is enabled
for IPv4 mDNS; multicast still depends on your Proxmox and LAN configuration.
