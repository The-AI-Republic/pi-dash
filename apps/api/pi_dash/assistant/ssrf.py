# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""SSRF guard for BYOK ``base_url`` values.

Enabled in cloud (``ASSISTANT_BLOCK_PRIVATE_URLS=True``), off by default in OSS
so self-hosters can point at a LAN vLLM/Ollama. See
``.ai_design/integrate_ai_agent/02-backend.md`` §7.
"""

from __future__ import annotations

import ipaddress
import socket
from urllib.parse import urlparse

from django.conf import settings


def blocking_enabled() -> bool:
    return bool(getattr(settings, "ASSISTANT_BLOCK_PRIVATE_URLS", False))


def is_blocked(url: str) -> bool:
    """True if ``url`` resolves to a private/loopback/link-local address."""
    if not blocking_enabled():
        return False
    host = urlparse(url).hostname
    if not host:
        return True
    try:
        infos = socket.getaddrinfo(host, None)
    except socket.gaierror:
        return True
    for info in infos:
        try:
            ip = ipaddress.ip_address(info[4][0])
        except ValueError:
            return True
        if ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved or ip.is_multicast:
            return True
    return False
