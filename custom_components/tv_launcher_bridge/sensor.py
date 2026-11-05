"""Read-only details: the app on screen and what's playing."""

from __future__ import annotations

from homeassistant.components.sensor import SensorEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .entity import BridgeEntity

SENSORS = [
    ("current_app", "Current app", "mdi:application"),
    ("media_title", "Now playing", "mdi:filmstrip"),
    ("media_state", "Playback", "mdi:play-circle-outline"),
]


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    async_add_entities(TvSensor(entry.runtime_data, *s) for s in SENSORS)


class TvSensor(BridgeEntity, SensorEntity):
    def __init__(self, coordinator, field: str, label: str, icon: str) -> None:
        super().__init__(coordinator, f"sensor_{field}")
        self._field = field
        self._attr_name = label
        self._attr_icon = icon

    @property
    def native_value(self) -> str | None:
        if not self.tv_on:
            return "off" if self._field in ("media_state", "current_app") else None
        value = self.status.get(self._field)
        if self._field == "media_state" and not value:
            return "idle"
        if self._field == "current_app" and not value:
            return "Home screen"
        return value or None
