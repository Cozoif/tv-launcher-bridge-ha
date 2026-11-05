"""Talks to the TV Launcher Bridge program over the home network."""

from __future__ import annotations

from typing import Any

import aiohttp


class BridgeError(Exception):
    """The bridge or the TV reported a problem."""


class BridgeAuthError(BridgeError):
    """Wrong or missing password."""


class BridgeApi:
    """Small client for the bridge's web addresses."""

    def __init__(self, session: aiohttp.ClientSession, host: str, port: int, token: str = "") -> None:
        self._session = session
        self._base = f"http://{host}:{port}"
        self._token = token.strip()

    async def _request(
        self,
        method: str,
        path: str,
        params: dict[str, Any] | None = None,
        json: dict[str, Any] | None = None,
        timeout: float = 30,
    ) -> dict[str, Any]:
        headers = {"Authorization": f"Bearer {self._token}"} if self._token else {}
        clean = {k: str(v) for k, v in (params or {}).items() if v is not None and v != ""}
        try:
            async with self._session.request(
                method,
                self._base + path,
                params=clean,
                json=json,
                headers=headers,
                timeout=aiohttp.ClientTimeout(total=timeout),
            ) as resp:
                data = await resp.json(content_type=None)
                if resp.status == 401:
                    raise BridgeAuthError(data.get("message", "wrong password"))
                if resp.status >= 400 or not data.get("ok", True):
                    raise BridgeError(data.get("message", f"error {resp.status}"))
                return data
        except (aiohttp.ClientError, TimeoutError) as err:
            raise BridgeError(f"can't reach TV Launcher Bridge at {self._base}: {err}") from err

    async def info(self) -> dict[str, Any]:
        return await self._request("GET", "/info", timeout=10)

    async def status(self) -> dict[str, Any]:
        return await self._request("GET", "/status", timeout=20)

    async def key(self, name: str, hold: bool = False) -> dict[str, Any]:
        return await self._request("POST", f"/key/{name}", {"hold": "1" if hold else None})

    async def launch(self, app: str) -> dict[str, Any]:
        return await self._request("POST", "/launch", {"app": app})

    async def close(self, app: str) -> dict[str, Any]:
        return await self._request("POST", "/close", {"app": app})

    async def power(self, on: bool) -> dict[str, Any]:
        return await self._request("POST", "/power/on" if on else "/power/off")

    async def link(self, url: str, app: str | None = None) -> dict[str, Any]:
        return await self._request("POST", "/link", {"url": url, "app": app})

    async def search(self, query: str, app: str | None = None) -> dict[str, Any]:
        return await self._request("POST", "/search", {"query": query, "app": app}, timeout=40)

    async def text(self, value: str) -> dict[str, Any]:
        return await self._request("POST", "/text", {"value": value})

    async def intent(self, intent: dict[str, Any]) -> dict[str, Any]:
        return await self._request("POST", "/intent", json=intent)

    async def watch(self, show: str, season: int | None = None, episode: int | None = None) -> dict[str, Any]:
        return await self._request(
            "POST", "/watch", {"show": show, "season": season, "episode": episode}, timeout=90
        )

    async def jellyfin_play(self, show: str, season: int | None = None, episode: int | None = None) -> dict[str, Any]:
        return await self._request(
            "POST", "/jellyfin/play", {"show": show, "season": season, "episode": episode}, timeout=40
        )

    async def adb(self, command: str) -> dict[str, Any]:
        return await self._request("POST", "/adb", json={"command": command})
