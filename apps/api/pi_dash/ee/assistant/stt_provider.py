# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE speech-to-text (dictation) provider seam.

Open source: every user brings their own OpenAI-compatible
``/v1/audio/transcriptions`` endpoint (base URL + API key + model), configured
in Settings — exactly like BYOK LLM config. The cloud build overlays this
module to route dictation to the OpenHub relay on the user's existing AI
Republic session instead, with no key to paste (see ``PDASHOSS01-148``).

The transcribe endpoint calls :func:`has_usable_stt_config` and
:func:`resolve_stt_provider` rather than reading :class:`UserSTTConfig`
directly, so the overlay is the single switch point — mirroring the LLM seam in
:mod:`pi_dash.ee.assistant.model_provider`.
"""

from __future__ import annotations

from dataclasses import dataclass

from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import BaseUrlBlocked, STTConfigMissing
from pi_dash.assistant.models import UserSTTConfig


@dataclass(frozen=True)
class ResolvedSTTProvider:
    """A ready-to-call transcription endpoint for one user.

    ``base_url`` is the OpenAI-compatible root (no trailing ``/audio/...``);
    ``api_key`` is the decrypted credential to send as a bearer token;
    ``model`` is the transcription model slug. The cloud overlay produces the
    same shape from an OpenHub gateway token, so callers never branch on build.
    """

    base_url: str
    api_key: str
    model: str


def has_usable_stt_config(user) -> bool:
    """True when ``user`` has a usable dictation configuration (CE: a BYO key).

    Cheap presence check for request-time gating (rejecting a transcribe call
    before streaming a doomed upload). Must not decrypt anything. The cloud
    overlay also accepts its platform credential here.
    """
    cfg = UserSTTConfig.objects.filter(user=user).first()
    return bool(cfg and cfg.has_api_key)


def resolve_stt_provider(user) -> ResolvedSTTProvider:
    """Resolve ``user``'s transcription endpoint (CE: their BYO STT config).

    Raises :class:`STTConfigMissing` when the user has no usable config,
    :class:`BaseUrlBlocked` when the configured host is refused by the SSRF
    guard, and :class:`~pi_dash.assistant.errors.AssistantError` when the
    stored key cannot be decrypted.

    The SSRF guard is re-run here, at execution time, rather than trusting the
    save-time check: DNS can be re-pointed at a private address after the URL
    was stored, so the guard must fire immediately before the outbound request.
    """
    cfg = UserSTTConfig.objects.filter(user=user).first()
    if cfg is None or not cfg.has_api_key:
        raise STTConfigMissing("Configure dictation in Settings.")
    if cfg.base_url and ssrf.is_blocked(cfg.base_url):
        raise BaseUrlBlocked("That endpoint host is not allowed.")
    api_key = crypto.decrypt(cfg.api_key_encrypted)
    return ResolvedSTTProvider(base_url=cfg.base_url, api_key=api_key, model=cfg.model_name)
