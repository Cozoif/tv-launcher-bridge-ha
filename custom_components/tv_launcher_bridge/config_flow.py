"""Setup screen: where is TV Launcher Bridge running?"""

from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigFlow, ConfigFlowResult
from homeassistant.const import CONF_HOST, CONF_PORT
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .api import BridgeApi, BridgeAuthError, BridgeError
from .const import CONF_TOKEN, DEFAULT_HOST, DEFAULT_PORT, DOMAIN


class TvLauncherBridgeConfigFlow(ConfigFlow, domain=DOMAIN):
    """Ask for the bridge's address."""

    VERSION = 1

    async def async_step_user(self, user_input: dict[str, Any] | None = None) -> ConfigFlowResult:
        errors: dict[str, str] = {}
        if user_input is not None:
            host = user_input[CONF_HOST].strip()
            port = int(user_input[CONF_PORT])
            api = BridgeApi(async_get_clientsession(self.hass), host, port, user_input.get(CONF_TOKEN, ""))
            try:
                info = await api.info()
            except BridgeAuthError:
                errors["base"] = "invalid_auth"
            except BridgeError:
                errors["base"] = "cannot_connect"
            else:
                await self.async_set_unique_id(f"{host}:{port}")
                self._abort_if_unique_id_configured()
                return self.async_create_entry(
                    title=info.get("name") or "Chromecast",
                    data={CONF_HOST: host, CONF_PORT: port, CONF_TOKEN: user_input.get(CONF_TOKEN, "")},
                )

        default_host = "" if DEFAULT_HOST.startswith("__") else DEFAULT_HOST
        schema = vol.Schema(
            {
                vol.Required(CONF_HOST, default=default_host): str,
                vol.Required(CONF_PORT, default=DEFAULT_PORT): int,
                vol.Optional(CONF_TOKEN, default=""): str,
            }
        )
        return self.async_show_form(step_id="user", data_schema=schema, errors=errors)

    async def async_step_reconfigure(self, user_input=None):
        """Move an existing entry to the HA add-on without replacing entities."""
        entry = self._get_reconfigure_entry()
        errors = {}
        if user_input is not None:
            api = BridgeApi(async_get_clientsession(self.hass), user_input[CONF_HOST].strip(),
                            int(user_input[CONF_PORT]), user_input.get(CONF_TOKEN, ""))
            try:
                await api.info()
            except BridgeAuthError:
                errors["base"] = "invalid_auth"
            except BridgeError:
                errors["base"] = "cannot_connect"
            else:
                return self.async_update_reload_and_abort(entry, data_updates=user_input)
        return self.async_show_form(step_id="reconfigure", errors=errors, data_schema=vol.Schema({
            vol.Required(CONF_HOST, default="127.0.0.1"): str,
            vol.Required(CONF_PORT, default=DEFAULT_PORT): int,
            vol.Required(CONF_TOKEN, default=entry.data.get(CONF_TOKEN, "")): str,
        }))
