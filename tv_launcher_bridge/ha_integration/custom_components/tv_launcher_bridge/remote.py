"""The TV as a remote: send any button, hold it, repeat it; apps as activities."""

from __future__ import annotations

import asyncio
from collections.abc import Iterable
from typing import Any

from homeassistant.components.remote import (
    ATTR_ACTIVITY,
    ATTR_DELAY_SECS,
    ATTR_HOLD_SECS,
    ATTR_NUM_REPEATS,
    DEFAULT_DELAY_SECS,
    RemoteEntity,
    RemoteEntityFeature,
)
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .api import BridgeError
from .entity import BridgeEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    async_add_entities([TvRemote(entry.runtime_data)])


class TvRemote(BridgeEntity, RemoteEntity):
    """remote.send_command with up, down, left, right, ok, back, home, play_pause, ..."""

    _attr_name = None
    _attr_supported_features = RemoteEntityFeature.ACTIVITY

    def __init__(self, coordinator) -> None:
        super().__init__(coordinator, "remote")

    @property
    def is_on(self) -> bool:
        return self.tv_on

    @property
    def activity_list(self) -> list[str]:
        return self.app_names

    @property
    def current_activity(self) -> str | None:
        return self.status.get("current_app") or None

    @property
    def extra_state_attributes(self) -> dict[str, Any]:
        return {"commands": [k["id"] for k in self.info.get("keys", [])]}

    async def async_turn_on(self, **kwargs: Any) -> None:
        api = self.coordinator.api
        await self.coordinator.run(api.power(True))
        if activity := kwargs.get(ATTR_ACTIVITY):
            await self.coordinator.run(api.launch(activity))

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self.coordinator.run(self.coordinator.api.power(False))

    async def async_send_command(self, command: Iterable[str], **kwargs: Any) -> None:
        api = self.coordinator.api
        repeats = int(kwargs.get(ATTR_NUM_REPEATS, 1))
        delay = float(kwargs.get(ATTR_DELAY_SECS, DEFAULT_DELAY_SECS))
        hold = float(kwargs.get(ATTR_HOLD_SECS, 0) or 0) >= 0.5
        try:
            for _ in range(repeats):
                for cmd in command:
                    # "launch:jellyfin" opens an app; "text:hello" types; anything else is a button.
                    if cmd.startswith("launch:"):
                        await api.launch(cmd.split(":", 1)[1])
                    elif cmd.startswith("text:"):
                        await api.text(cmd.split(":", 1)[1])
                    else:
                        await api.key(cmd, hold)
                    await asyncio.sleep(delay)
        except BridgeError as err:
            raise HomeAssistantError(str(err)) from err
        await self.coordinator.async_request_refresh()
