# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""WebSocket upgrade endpoints.

Per ``.ai_design/move_to_https/design.md`` §7.9, the WS protocol is
retired as the always-on control plane. The Channels consumer here is
kept only for the per-run "upgrade ticket" handshake — a future
opt-in stream for heavy traffic (live log tail, large event output).

The control-plane WebSocket
(``Authorization: Bearer <connection_secret>`` + ``X-Connection-Id``)
is gone; calls to that path are rejected with WS close code 1008
``protocol_version_unsupported``.
"""

from __future__ import annotations

import json
import logging
from typing import Any, Optional

from channels.generic.websocket import AsyncJsonWebsocketConsumer

logger = logging.getLogger(__name__)


CLOSE_CODE_PROTOCOL_UNSUPPORTED = 1008
CLOSE_CODE_TICKET_INVALID = 4404


class RunnerConsumer(AsyncJsonWebsocketConsumer):
    """Stub consumer that rejects all control-plane WebSocket traffic.

    Reserved for the per-run upgrade-ticket flow once that ships.
    """

    async def connect(self) -> None:
        # All control traffic is now over HTTPS long-poll; no WS allowed
        # without a per-run upgrade ticket.
        await self.close(code=CLOSE_CODE_PROTOCOL_UNSUPPORTED)

    async def disconnect(self, code: int) -> None:  # pragma: no cover - trivial
        return None

    async def receive_json(self, content: Any, **_: Any) -> None:  # pragma: no cover
        return None

    async def encode_json(self, content: Any) -> str:  # pragma: no cover
        return json.dumps(content, default=str)

    @staticmethod
    def _header(headers, name: str) -> Optional[str]:  # pragma: no cover - helper
        for key, value in headers or []:
            if key.decode().lower() == name:
                return value.decode()
        return None
