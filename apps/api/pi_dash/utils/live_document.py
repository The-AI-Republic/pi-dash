# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Server-side access to the live server's HTML → collaborative-document conversion.

Pages (and work-item descriptions) are collaborative Yjs documents.
``description_binary`` is the source of truth for the live editor; when it is
non-empty the live server never looks at ``description_html`` again
(``apps/live/src/extensions/database.ts``). So any server-side write of a
body must store HTML, ProseMirror JSON and the Yjs binary *together*, and only
the live server can produce the latter two — Python has no Tiptap schema.

:func:`convert_document` wraps ``POST {LIVE_URL}/convert-document/``. Unlike
the historical best-effort caller in ``bgtasks/copy_s3_object.py`` it raises
:class:`LiveConversionError` whenever the conversion is unavailable, so a
caller can fail the write instead of saving HTML that the editor would then
silently discard.

Pass ``base_binary`` (the page's current ``description_binary``) whenever one
exists: the live server then applies the new content as an edit on top of the
existing document instead of building a fresh one. A fresh document has an
unrelated Yjs history, and a browser holding the old document in its
IndexedDB cache would merge the two into duplicated content.

The helper is pure — no database access, no request state — so the API views
and the downstream MCP connector can share it.
"""

from __future__ import annotations

import base64
import binascii
from dataclasses import dataclass

import requests
from django.conf import settings

from pi_dash.utils.url import normalize_url_path

#: Seconds to wait for the live server. A conversion is CPU-only on the live
#: side, so anything slower than this means the service is unhealthy.
LIVE_CONVERSION_TIMEOUT = 15

#: Editor variant for project pages. The live server loads pages with the
#: document-editor schema (it carries work-item embeds and a title fragment
#: that the rich-text schema lacks), so converting with ``"rich"`` would drop
#: those nodes.
PAGE_VARIANT = "document"


class LiveConversionError(Exception):
    """The live server could not produce the collaborative document."""


@dataclass(frozen=True)
class LiveDocument:
    description_html: str
    description_json: dict
    description_binary: bytes


def convert_document(
    description_html: str,
    *,
    variant: str = PAGE_VARIANT,
    base_binary: bytes | None = None,
    title: str | None = None,
) -> LiveDocument:
    """Convert HTML into all three stored formats via the live server.

    ``base_binary`` is the current Yjs state to apply the change onto, and
    ``title`` (document variant only) rewrites the document's title fragment
    so the editor does not show a stale name after a rename.

    Returns the HTML as the editor re-serialises it when the live server
    reports it, falling back to the HTML that was sent.

    Raises :class:`LiveConversionError` when ``LIVE_URL`` is unset, the live
    server is unreachable or answers with an error, or the response is not a
    usable document.
    """
    live_url = getattr(settings, "LIVE_URL", None)
    if not live_url:
        raise LiveConversionError("LIVE_URL is not configured, so the page document cannot be regenerated")

    payload = {"description_html": description_html or "<p></p>", "variant": variant}
    if base_binary:
        payload["description_binary"] = base64.b64encode(bytes(base_binary)).decode("ascii")
    if title is not None:
        payload["title"] = title

    url = normalize_url_path(f"{live_url}/convert-document/")
    try:
        response = requests.post(url, json=payload, timeout=LIVE_CONVERSION_TIMEOUT)
    except requests.RequestException as exc:
        raise LiveConversionError(f"the live server is unreachable: {exc.__class__.__name__}") from exc

    if response.status_code != 200:
        raise LiveConversionError(f"the live server rejected the conversion (HTTP {response.status_code})")

    try:
        data = response.json()
        encoded_binary = data["description_binary"]
        description_binary = base64.b64decode(encoded_binary, validate=True)
    except (ValueError, KeyError, TypeError, binascii.Error) as exc:
        raise LiveConversionError("the live server returned an unusable document") from exc
    if not description_binary:
        raise LiveConversionError("the live server returned an empty document")

    return LiveDocument(
        description_html=data.get("description_html") or payload["description_html"],
        description_json=data.get("description_json") or {},
        description_binary=description_binary,
    )
