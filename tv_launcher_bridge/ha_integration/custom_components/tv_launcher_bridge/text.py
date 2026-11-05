"""Text boxes: play a show, search, type, open a link — type and press enter."""

from __future__ import annotations

from homeassistant.components.text import TextEntity, TextMode
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .entity import BridgeEntity

BOXES = [
    ("play_show", "Play show on Jellyfin", "mdi:play-box-multiple"),
    ("search", "Search on TV", "mdi:magnify"),
    ("type_text", "Type on TV", "mdi:keyboard"),
    ("open_link", "Open link", "mdi:link-variant"),
]


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    async_add_entities(TvTextBox(entry.runtime_data, *b) for b in BOXES)


class TvTextBox(BridgeEntity, TextEntity):
    _attr_mode = TextMode.TEXT
    _attr_native_max = 255

    def __init__(self, coordinator, kind: str, label: str, icon: str) -> None:
        super().__init__(coordinator, f"text_{kind}")
        self._kind = kind
        self._attr_name = label
        self._attr_icon = icon
        self._attr_native_value = ""

    async def async_set_value(self, value: str) -> None:
        api = self.coordinator.api
        value = value.strip()
        self._attr_native_value = value
        self.async_write_ha_state()
        if not value:
            return
        if self._kind == "play_show":
            await self.coordinator.run(api.watch(value))
        elif self._kind == "search":
            await self.coordinator.run(api.search(value))
        elif self._kind == "type_text":
            await self.coordinator.run(api.text(value))
        elif self._kind == "open_link":
            await self.coordinator.run(api.link(value))
