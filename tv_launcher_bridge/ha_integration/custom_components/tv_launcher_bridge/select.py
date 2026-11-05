"""Pick an app from a list to open it on the TV."""

from __future__ import annotations

from homeassistant.components.select import SelectEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .entity import BridgeEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    async_add_entities([TvAppSelect(entry.runtime_data)])


class TvAppSelect(BridgeEntity, SelectEntity):
    _attr_name = "App"
    _attr_icon = "mdi:apps"

    def __init__(self, coordinator) -> None:
        super().__init__(coordinator, "app")

    @property
    def options(self) -> list[str]:
        names = self.app_names
        current = self.status.get("current_app")
        if current and current not in names:
            names = [*names, current]
        return names or ["Home"]

    @property
    def current_option(self) -> str | None:
        current = self.status.get("current_app")
        return current if current in self.options else None

    async def async_select_option(self, option: str) -> None:
        await self.coordinator.run(self.coordinator.api.launch(option))
