"""Shared base for all TV Launcher Bridge entities."""

from __future__ import annotations

from typing import Any

from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.update_coordinator import CoordinatorEntity

from . import BridgeCoordinator
from .const import DOMAIN


class BridgeEntity(CoordinatorEntity[BridgeCoordinator]):
    """One TV = one device in Home Assistant."""

    _attr_has_entity_name = True

    def __init__(self, coordinator: BridgeCoordinator, key: str) -> None:
        super().__init__(coordinator)
        entry = coordinator.entry
        self._attr_unique_id = f"{entry.entry_id}_{key}"
        model = (coordinator.data or {}).get("status", {}).get("model") or "Android TV"
        self._attr_device_info = DeviceInfo(
            identifiers={(DOMAIN, entry.entry_id)},
            name=entry.title,
            manufacturer="Google",
            model=model,
            configuration_url=f"http://{entry.data['host']}:{entry.data['port']}/",
        )

    @property
    def status(self) -> dict[str, Any]:
        return (self.coordinator.data or {}).get("status", {})

    @property
    def info(self) -> dict[str, Any]:
        return (self.coordinator.data or {}).get("info", {})

    @property
    def tv_on(self) -> bool:
        s = self.status
        return bool(s.get("connected")) and s.get("awake") is not False

    @property
    def app_names(self) -> list[str]:
        return [a["name"] for a in self.info.get("apps", [])]
