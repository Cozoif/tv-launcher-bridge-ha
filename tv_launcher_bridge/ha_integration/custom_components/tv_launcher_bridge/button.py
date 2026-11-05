"""One button per remote key, so every control shows up on the device page."""

from __future__ import annotations

from homeassistant.components.button import ButtonEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .const import KEY_ICONS
from .entity import BridgeEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    keys = (coordinator.data or {}).get("info", {}).get("keys", [])
    async_add_entities(TvKeyButton(coordinator, k["id"], k["label"]) for k in keys)


class TvKeyButton(BridgeEntity, ButtonEntity):
    def __init__(self, coordinator, key_id: str, label: str) -> None:
        super().__init__(coordinator, f"key_{key_id}")
        self._key = key_id
        self._attr_name = label
        self._attr_icon = KEY_ICONS.get(key_id, "mdi:remote")

    async def async_press(self) -> None:
        await self.coordinator.run(self.coordinator.api.key(self._key))
