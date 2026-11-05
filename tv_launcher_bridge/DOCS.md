# TV Launcher Bridge 0.3.0 — local HA OS installation (amd64)

The bridge runs in the HA OS VM, independently of your PC. Your existing Google Cast integration stays installed and unchanged. No APK is required. Do not forward ports 8125 or 8099 through your router.

## Files

- `tv-launcher-bridge-addon-amd64-source.zip`: complete local add-on repository; contains `repository.yaml` and `tv_launcher_bridge/` with its Dockerfile, schema, startup, UI, Rust sources, lockfile and bundled integration sources.
- `tv-launcher-bridge-ha.zip`: updated custom integration, with `custom_components/tv_launcher_bridge/` at its root.
- `INSTALL-HA-OS.md`: these instructions.
- `VALIDATION.txt`: local results and checks still required.

The editable source is also available in `app/ha_addon/tv_launcher_bridge/`. Run `python3 app/package_ha.py` from the project root to refresh the bundled Rust sources and ZIPs after editing.

## Copy and install the local add-on

1. In HA, install/configure the Samba share add-on, enable its `addons` share, and start it. Open `\\homeassistant\addons` from Windows (or `smb://HOME_ASSISTANT_IP/addons` on another system). Alternatively use an HA OS host console to copy into `/addons`; do not copy into `/config/addons`.
2. Extract `tv-launcher-bridge-addon-amd64-source.zip` on your computer. Copy the entire `tv_launcher_bridge` folder into that share. The final HA OS path must be `/addons/tv_launcher_bridge/config.yaml`, with `/addons/tv_launcher_bridge/Dockerfile` next to it. `repository.yaml` may be copied to `/addons/repository.yaml`; it is not required for local discovery. No GitHub account or repository URL is needed.
3. Open Settings → Add-ons → Add-on Store, select the three-dot menu → Check for updates, then refresh. Under Local add-ons select TV Launcher Bridge and Install. This builds the image locally on your Intel amd64 HA OS VM. It downloads Rust build dependencies and Debian packages; internet is needed for the build, not for ADB control. Allow sufficient RAM and free disk space (several GB).
4. Open the add-on Configuration tab. Set `api_token` to a unique random secret of at least 24 characters. Keep `api_bind: 127.0.0.1`. Optionally enter Jellyfin server URL (including `http://` or `https://`), API key and device name. Raw shell commands are disabled by default.
5. Save, start the add-on, enable Start on boot and Watchdog in HA if offered, and inspect Logs. Select Open Web UI for the Ingress pairing page. No desktop window or display server is used by this image.

Optional command-line build outside HA, on an amd64 Docker host:

```sh
cd tv_launcher_bridge
docker build --platform linux/amd64 -t tv-launcher-bridge-addon:0.3.0 .
```

Supervisor builds directly from the same Dockerfile; no prebuilt image registry is required. Leave Protection mode enabled. This add-on needs no privileged devices or Docker socket.

## Pair with the Chromecast

1. On the TV enable Developer options (Settings → System → About → Android TV OS build, press seven times), then Developer options → Wireless debugging. Permit debugging on your home network.
2. On the main Wireless debugging page note the TV IPv4 address and **connection port** shown in “IP address & Port”. It is often a high random port, not 5555. In Open Web UI enter those values. Save address and connection port. Discover wireless ADB can populate the current address/port; inspect its response, then reload the page to show the saved values.
3. On the TV select Pair device with pairing code. Keep that dialog open. Enter its **pairing port** and six-digit code in the add-on UI. The pairing port differs from the connection port. Press Pair. Codes are sent to the bridge without being saved or logged.
4. Press Connect / reconnect, then Check TV status. A pairing result only confirms pairing; successful status/connect responses are separate checks. Press Discover installed apps to replace the initial app list with the TV's actual launcher apps.
5. If the code expires, reopen the TV pairing dialog and retry with its new port/code. If multicast is unavailable, manually enter the current connection port and save. Do not enter the pairing port as the connection port.

## Install or update the custom integration

1. Extract `tv-launcher-bridge-ha.zip`. Copy `custom_components/tv_launcher_bridge` into the HA configuration share, yielding `/config/custom_components/tv_launcher_bridge/manifest.json`. Replace files from the old version; retain other integration folders.
2. Restart Home Assistant Core. Settings → Devices & services → Add integration → TV Launcher Bridge. Use host `127.0.0.1`, port `8125`, and the exact API token from add-on Configuration.
3. For an existing TV Launcher Bridge entry, use its three-dot menu → Reconfigure to change the host to `127.0.0.1` and token. This updates the existing entry and retains entity IDs, devices and actions. Do not delete the entry. Existing raw command permission remains controlled by the add-on option.
4. Existing media player, remote, buttons, app select, text and status entities and `tv_launcher_bridge` actions continue using the original authenticated API. After discovering apps, reload the integration to refresh app controls. Leave the Cast integration alone.

## Persistence and reconnection

ADB identity is stored in `/data/tv-launcher-bridge/adbkey.pem` inside the add-on. Settings, selected TV, discovered port and pairing name are in `/data/tv-launcher-bridge/settings.json`. The Supervisor preserves `/data` across restarts and updates. Include this add-on in HA backups. Uninstalling it or removing its data may require pairing again. Add-on options override token/Jellyfin/raw-command settings on startup; they do not overwrite the paired TV address or identity.

Connections open on demand when an action or HA status poll runs. Each shell request permits at most two attempts. Each connection first tries the saved port, then performs a roughly 2.5-second mDNS discovery and makes one additional connection attempt if the target TV advertises another port. ADB handshake negotiation is limited to 12 packets, with socket timeouts; this is bounded retry behavior, not a guarantee of connection or a strict total request deadline. HA may time out before a slow TV handshake finishes; the next poll/action retries. Pairing is never retried with stored codes. Errors appear in status/UI and the integration becomes unavailable when the TV cannot be reached. Sleep, Wi-Fi loss, disabling debugging or revoking authorization may require waking the TV or pairing again.

## Networking and actual limitations

`host_network: true` lets the existing IPv4 mDNS discovery use multicast UDP 5353 on HA OS network interfaces. The UI uses port 8099 and rejects every socket peer except the Supervisor Ingress address `172.30.32.2`, following [HA Ingress guidance](https://developers.home-assistant.io/docs/apps/presentation/). API port 8125 binds only to loopback by default; HA Core on HA OS uses the host network. The API requires the configured token. No public service is created.

Use a bridged Proxmox VM network on the same LAN/VLAN as the Chromecast. VM/firewall rules must allow multicast 224.0.0.251 UDP 5353 and TCP connections from HA to the TV's advertised debugging ports. Wi-Fi client isolation, multicast filtering/IGMP rules, NAT and VLAN boundaries can prevent discovery. Host networking cannot bypass them. Discovery currently resolves IPv4 A/SRV records, not IPv6-only TVs; multiple NICs may require network routing adjustments. A manual current connection port works when mDNS is blocked, but automatic discovery after port changes then cannot work until multicast is restored. Ports 8125 and 8099 must be unused on the HA host.

The optional existing TV companion APK accepts a bridge URL and token. To use it, set `api_bind: 0.0.0.0`, restart the add-on and configure the companion URL as `http://HA_VM_LAN_IP:8125` with the same token. This exposes the token-protected API to your trusted LAN over HTTP; use only on that LAN and never forward it to the internet. The Ingress UI still rejects direct LAN requests. Keep loopback binding if you do not need the companion.

## Test on HA OS and the actual TV

1. Verify Connect and Check TV status report the intended Chromecast, then discover apps. In HA launch YouTube via the source selector and a launch action; exercise Home/Back/navigation, play/pause, volume and power.
2. Open a TV search field and test Type text. Test an app-supported link and a send-intent action. ADB text input retains its existing keyboard/Unicode limitations.
3. If using Jellyfin, log the TV app into your server, configure the URL/key/device option, then test a known film/show through Play show. Test plain casting separately to confirm it still works.
4. Restart the add-on and verify control without pairing again. Rebuild/reinstall the add-on using its update/rebuild control and verify identity survives. Restart HA OS and repeat.
5. Toggle TV wireless debugging to change its connection port. Retry status; verify the new discovered port is saved. Turn off the TV Wi-Fi and confirm a clear failure, restore it and retry. Revoke pairing on the TV and verify errors until re-paired.
6. Switch the PC completely off and run the same HA actions. This is the acceptance check for PC independence. No successful Chromecast operation or permanent connectivity is claimed by the local build tests.
