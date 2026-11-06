"""TV Launcher Bridge: control a Chromecast with Google TV (any Android TV) from Home Assistant."""

from __future__ import annotations

from datetime import timedelta
import logging
from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigEntry, ConfigEntryState
from homeassistant.const import CONF_HOST, CONF_PORT, Platform
from homeassistant.core import HomeAssistant, ServiceCall, ServiceResponse, SupportsResponse
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers import config_validation as cv
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.typing import ConfigType
from homeassistant.helpers.update_coordinator import DataUpdateCoordinator, UpdateFailed

from .api import BridgeApi, BridgeError
from .const import CONF_TOKEN, DOMAIN, SCAN_SECONDS

_LOGGER = logging.getLogger(__name__)

PLATFORMS = [
    Platform.MEDIA_PLAYER,
    Platform.REMOTE,
    Platform.BUTTON,
    Platform.SELECT,
    Platform.TEXT,
    Platform.SENSOR,
]

CONFIG_SCHEMA = cv.config_entry_only_config_schema(DOMAIN)


class BridgeCoordinator(DataUpdateCoordinator[dict[str, Any]]):
    """Asks the bridge what the TV is doing every few seconds."""

    def __init__(self, hass: HomeAssistant, entry: ConfigEntry, api: BridgeApi) -> None:
        super().__init__(
            hass,
            _LOGGER,
            name=f"{DOMAIN} {entry.title}",
            update_interval=timedelta(seconds=SCAN_SECONDS),
            config_entry=entry,
        )
        self.api = api
        self.entry = entry

    async def _async_update_data(self) -> dict[str, Any]:
        try:
            info = await self.api.info()
        except BridgeError as err:
            raise UpdateFailed(str(err)) from err
        try:
            status = await self.api.status()
        except BridgeError as err:
            # The bridge is fine but the TV isn't answering (often: it's off).
            status = {"connected": False, "awake": False, "message": str(err)}
        return {"info": info, "status": status}

    async def run(self, coro) -> dict[str, Any]:
        """Run a command, turn problems into readable errors, then refresh."""
        try:
            result = await coro
        except BridgeError as err:
            raise HomeAssistantError(str(err)) from err
        await self.async_request_refresh()
        return result


type BridgeConfigEntry = ConfigEntry[BridgeCoordinator]


async def async_setup_entry(hass: HomeAssistant, entry: BridgeConfigEntry) -> bool:
    api = BridgeApi(
        async_get_clientsession(hass),
        entry.data[CONF_HOST],
        entry.data[CONF_PORT],
        entry.data.get(CONF_TOKEN, ""),
    )
    coordinator = BridgeCoordinator(hass, entry, api)
    await coordinator.async_config_entry_first_refresh()
    entry.runtime_data = coordinator
    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)
    return True


async def async_unload_entry(hass: HomeAssistant, entry: BridgeConfigEntry) -> bool:
    return await hass.config_entries.async_unload_platforms(entry, PLATFORMS)


# ---------------------------------------------------------------------------
# Actions (services) for commands that aren't a plain button press
# ---------------------------------------------------------------------------

ENTRY = vol.Optional("config_entry_id")

SCHEMAS = {
    "launch_app": vol.Schema({ENTRY: cv.string, vol.Required("app"): cv.string}),
    "close_app": vol.Schema({ENTRY: cv.string, vol.Required("app"): cv.string}),
    "send_key": vol.Schema(
        {ENTRY: cv.string, vol.Required("key"): cv.string, vol.Optional("hold", default=False): cv.boolean}
    ),
    "play_sitcom": vol.Schema({ENTRY: cv.string}),
    "play_show": vol.Schema(
        {
            ENTRY: cv.string,
            vol.Required("show"): cv.string,
            vol.Optional("season"): vol.Coerce(int),
            vol.Optional("episode"): vol.Coerce(int),
            vol.Optional("turn_on", default=True): cv.boolean,
        }
    ),
    "open_link": vol.Schema({ENTRY: cv.string, vol.Required("url"): cv.string, vol.Optional("app"): cv.string}),
    "search": vol.Schema({ENTRY: cv.string, vol.Required("query"): cv.string, vol.Optional("app"): cv.string}),
    "type_text": vol.Schema({ENTRY: cv.string, vol.Required("text"): cv.string}),
    "send_intent": vol.Schema(
        {
            ENTRY: cv.string,
            vol.Optional("action"): cv.string,
            vol.Optional("data"): cv.string,
            vol.Optional("package"): cv.string,
            vol.Optional("component"): cv.string,
            vol.Optional("category"): cv.string,
            vol.Optional("mime_type"): cv.string,
            vol.Optional("extras", default={}): dict,
        }
    ),
    "adb_command": vol.Schema({ENTRY: cv.string, vol.Required("command"): cv.string}),
}


def _targets(hass: HomeAssistant, call: ServiceCall) -> list[BridgeCoordinator]:
    wanted = call.data.get("config_entry_id")
    found = [
        e.runtime_data
        for e in hass.config_entries.async_entries(DOMAIN)
        if e.state is ConfigEntryState.LOADED and wanted in (None, e.entry_id)
    ]
    if not found:
        raise HomeAssistantError("No TV Launcher Bridge is set up (or the id is wrong)")
    return found


async def async_setup(hass: HomeAssistant, config: ConfigType) -> bool:
    async def handle(call: ServiceCall) -> ServiceResponse:
        d = call.data
        results: dict[str, Any] = {}
        for c in _targets(hass, call):
            api = c.api
            match call.service:
                case "launch_app":
                    r = await c.run(api.launch(d["app"]))
                case "close_app":
                    r = await c.run(api.close(d["app"]))
                case "send_key":
                    r = await c.run(api.key(d["key"], d["hold"]))
                case "play_sitcom":
                    r = await c.run(api.watch_sitcom())
                case "play_show":
                    if d["turn_on"]:
                        r = await c.run(api.watch(d["show"], d.get("season"), d.get("episode")))
                    else:
                        r = await c.run(api.jellyfin_play(d["show"], d.get("season"), d.get("episode")))
                case "open_link":
                    r = await c.run(api.link(d["url"], d.get("app")))
                case "search":
                    r = await c.run(api.search(d["query"], d.get("app")))
                case "type_text":
                    r = await c.run(api.text(d["text"]))
                case "send_intent":
                    intent = {k: v for k, v in d.items() if k != "config_entry_id"}
                    r = await c.run(api.intent(intent))
                case "adb_command":
                    r = await c.run(api.adb(d["command"]))
                case _:
                    continue
            results[c.entry.entry_id] = r
        return {"results": results} if call.return_response else None

    for name, schema in SCHEMAS.items():
        hass.services.async_register(
            DOMAIN, name, handle, schema=schema, supports_response=SupportsResponse.OPTIONAL
        )
    return True
