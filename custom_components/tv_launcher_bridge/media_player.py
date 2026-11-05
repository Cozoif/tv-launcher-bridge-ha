"""The TV as a media player: power, playback, volume, apps as sources, play anything."""

from __future__ import annotations

from typing import Any

from homeassistant.components.media_player import (
    MediaPlayerDeviceClass,
    MediaPlayerEntity,
    MediaPlayerEntityFeature,
    MediaPlayerState,
)
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from . import BridgeConfigEntry
from .entity import BridgeEntity

F = MediaPlayerEntityFeature


async def async_setup_entry(
    hass: HomeAssistant, entry: BridgeConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    async_add_entities([TvMediaPlayer(entry.runtime_data)])


class TvMediaPlayer(BridgeEntity, MediaPlayerEntity):
    """Everything a TV remote's media keys do, plus opening apps and playing shows."""

    _attr_name = None
    _attr_device_class = MediaPlayerDeviceClass.TV
    _attr_supported_features = (
        F.TURN_ON
        | F.TURN_OFF
        | F.VOLUME_STEP
        | F.VOLUME_MUTE
        | F.PLAY
        | F.PAUSE
        | F.STOP
        | F.NEXT_TRACK
        | F.PREVIOUS_TRACK
        | F.SELECT_SOURCE
        | F.PLAY_MEDIA
    )

    def __init__(self, coordinator) -> None:
        super().__init__(coordinator, "media_player")

    @property
    def state(self) -> MediaPlayerState | None:
        if not self.tv_on:
            return MediaPlayerState.OFF
        media = self.status.get("media_state")
        if media == "playing":
            return MediaPlayerState.PLAYING
        if media == "paused":
            return MediaPlayerState.PAUSED
        if media == "buffering":
            return MediaPlayerState.BUFFERING
        return MediaPlayerState.ON

    @property
    def source(self) -> str | None:
        return self.status.get("current_app") or None

    @property
    def source_list(self) -> list[str]:
        return self.app_names

    @property
    def app_id(self) -> str | None:
        return self.status.get("current_package") or None

    @property
    def app_name(self) -> str | None:
        return self.status.get("current_app") or None

    @property
    def media_title(self) -> str | None:
        return self.status.get("media_title") or None

    @property
    def media_series_title(self) -> str | None:
        return self.status.get("media_subtitle") or None

    @property
    def extra_state_attributes(self) -> dict[str, Any]:
        s = self.status
        return {
            "connected": s.get("connected"),
            "encrypted_connection": s.get("encrypted"),
            "media_app": s.get("media_app"),
            "last_message": s.get("message"),
        }

    async def async_turn_on(self) -> None:
        await self.coordinator.run(self.coordinator.api.power(True))

    async def async_turn_off(self) -> None:
        await self.coordinator.run(self.coordinator.api.power(False))

    async def async_volume_up(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("volume_up"))

    async def async_volume_down(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("volume_down"))

    async def async_mute_volume(self, mute: bool) -> None:
        await self.coordinator.run(self.coordinator.api.key("mute"))

    async def async_media_play(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("play"))

    async def async_media_pause(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("pause"))

    async def async_media_stop(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("stop"))

    async def async_media_next_track(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("next"))

    async def async_media_previous_track(self) -> None:
        await self.coordinator.run(self.coordinator.api.key("previous"))

    async def async_select_source(self, source: str) -> None:
        await self.coordinator.run(self.coordinator.api.launch(source))

    async def async_play_media(self, media_type: str, media_id: str, **kwargs: Any) -> None:
        """media_type: app, url, show (Jellyfin), search, text — or a link/app by guess."""
        api = self.coordinator.api
        kind = (media_type or "").lower()
        if kind in ("app", "source"):
            await self.coordinator.run(api.launch(media_id))
        elif "://" in media_id:
            await self.coordinator.run(api.link(media_id))
        elif kind in ("show", "jellyfin", "tvshow", "movie", "episode", "video"):
            await self.coordinator.run(api.watch(media_id))
        elif kind == "search":
            await self.coordinator.run(api.search(media_id))
        elif kind == "text":
            await self.coordinator.run(api.text(media_id))
        else:
            await self.coordinator.run(api.launch(media_id))
