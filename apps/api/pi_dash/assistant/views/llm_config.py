# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from asgiref.sync import async_to_sync
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import ProviderKind, UserLLMConfig
from pi_dash.assistant.runtime.llm import build_model, resolve_byok_model
from pi_dash.assistant.serializers import UserLLMConfigSerializer

# Work-item titles are capped at 255 chars by the model; keep the AI-generated
# one comfortably inside a single readable line.
_TITLE_MAX_LEN = 255
_TITLE_SYSTEM_PROMPT = (
    "You write concise, specific titles for project work items. "
    "Given a work item's description, reply with a single short title "
    "(at most 80 characters) that captures what it is about. "
    "Return only the title text: no surrounding quotes, no trailing "
    "punctuation, and no preamble such as 'Title:'."
)


def _serialize(cfg: UserLLMConfig | None) -> dict:
    if cfg is None:
        return {
            "provider_kind": ProviderKind.OPENAI_COMPATIBLE,
            "base_url": "",
            "model_name": "",
            "has_api_key": False,
            "last_verified_at": None,
        }
    return {
        "provider_kind": cfg.provider_kind,
        "base_url": cfg.base_url,
        "model_name": cfg.model_name,
        "has_api_key": cfg.has_api_key,
        "last_verified_at": cfg.last_verified_at.isoformat() if cfg.last_verified_at else None,
    }


class UserLLMConfigEndpoint(BaseAPIView):
    def get(self, request):
        cfg = UserLLMConfig.objects.filter(user=request.user).first()
        return Response(_serialize(cfg))

    def put(self, request):
        cfg = UserLLMConfig.objects.filter(user=request.user).first()
        serializer = UserLLMConfigSerializer(instance=cfg, data=request.data, partial=cfg is not None)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        base_url = data.get("base_url", cfg.base_url if cfg else "")
        if base_url and ssrf.is_blocked(base_url):
            return Response(
                {"error": "base_url_blocked", "detail": "That endpoint host is not allowed."},
                status=status.HTTP_400_BAD_REQUEST,
            )

        api_key = data.pop("api_key", None)
        if cfg is None:
            cfg = UserLLMConfig(user=request.user)
        for field in ("provider_kind", "base_url", "model_name"):
            if field in data:
                setattr(cfg, field, data[field])
        if api_key:
            try:
                cfg.api_key_encrypted = crypto.encrypt(api_key)
            except AssistantError as exc:
                return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)
        try:
            cfg.save()
        except Exception:
            return Response({"error": "invalid"}, status=status.HTTP_400_BAD_REQUEST)
        return Response(_serialize(cfg))

    def delete(self, request):
        UserLLMConfig.objects.filter(user=request.user).delete()
        return Response(status=status.HTTP_204_NO_CONTENT)


class UserLLMConfigTestThrottle(UserRateThrottle):
    scope = "assistant_llm_test"


class UserLLMConfigTestEndpoint(BaseAPIView):
    throttle_classes = [UserLLMConfigTestThrottle]

    def post(self, request):
        cfg = UserLLMConfig.objects.filter(user=request.user).first()
        if cfg is None or not cfg.has_api_key:
            return Response({"ok": False, "error_code": "llm_config_missing"})
        if cfg.base_url and ssrf.is_blocked(cfg.base_url):
            return Response({"ok": False, "error_code": "base_url_blocked"})
        try:
            api_key = crypto.decrypt(cfg.api_key_encrypted)
        except AssistantError as exc:
            return Response({"ok": False, "error_code": exc.code})

        try:
            ok, code, detail = _run_test(cfg, api_key)
        except Exception:  # noqa: BLE001 — never echo raw errors (may reveal internal hosts)
            return Response({"ok": False, "error_code": "provider_unreachable"})

        if ok:
            cfg.last_verified_at = timezone.now()
            cfg.save(update_fields=["last_verified_at"])
            return Response({"ok": True})
        return Response({"ok": False, "error_code": code, "detail": detail})


class UserLLMConfigGenerateTitleThrottle(UserRateThrottle):
    scope = "assistant_llm_generate_title"


class UserLLMConfigGenerateTitleEndpoint(BaseAPIView):
    """Generate a work-item title from its description using the user's BYOK model.

    Used by the create-issue modal when the user leaves the title blank but has an
    AI assistant configured. Errors are reported with the same stable machine codes
    as the rest of the assistant surface so the client can react uniformly.
    """

    throttle_classes = [UserLLMConfigGenerateTitleThrottle]

    def post(self, request):
        # Coerce to str before .strip(): an untrusted payload may send a
        # non-string description (e.g. a number), which would otherwise 500.
        description = str(request.data.get("description") or "").strip()
        if not description:
            return Response(
                {"error": "description_required", "detail": "A description is required to generate a title."},
                status=status.HTTP_400_BAD_REQUEST,
            )

        cfg = UserLLMConfig.objects.filter(user=request.user).first()
        if cfg is None or not cfg.has_api_key:
            return Response(
                {"error": "llm_config_missing", "detail": "No AI assistant is configured."},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )

        try:
            model = resolve_byok_model(request.user)
        except AssistantError as exc:
            return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)

        try:
            title = _generate_title(model, description)
        except Exception:  # noqa: BLE001 — never echo raw errors (may reveal internal hosts)
            return Response(
                {"error": "provider_unreachable", "detail": "Could not reach the AI provider."},
                status=status.HTTP_502_BAD_GATEWAY,
            )

        if not title:
            return Response(
                {"error": "generation_failed", "detail": "The AI assistant did not return a usable title."},
                status=status.HTTP_502_BAD_GATEWAY,
            )
        return Response({"title": title})


def _generate_title(model, description: str) -> str:
    from pydantic_ai import Agent, UsageLimits

    agent = Agent(model=model, system_prompt=_TITLE_SYSTEM_PROMPT)

    async def _go():
        result = await agent.run(description, usage_limits=UsageLimits(request_limit=1))
        return result.output

    return _clean_title(async_to_sync(_go)())


def _clean_title(raw: str) -> str:
    """Normalize the model's reply into a single-line title within the length cap."""
    title = (raw or "").strip()
    if not title:
        return ""
    # The model occasionally wraps the title in quotes or spreads it over lines.
    title = title.splitlines()[0].strip().strip("\"'").strip()
    if len(title) > _TITLE_MAX_LEN:
        title = title[:_TITLE_MAX_LEN].rstrip()
    return title


def _run_test(cfg: UserLLMConfig, api_key: str) -> tuple[bool, str, str]:
    from pydantic_ai import Agent, UsageLimits

    model = build_model(
        provider_kind=cfg.provider_kind,
        base_url=cfg.base_url,
        model_name=cfg.model_name,
        api_key=api_key,
    )
    agent = Agent(model=model)

    async def _go():
        await agent.run("Reply with the single word: ok", usage_limits=UsageLimits(request_limit=1))

    try:
        async_to_sync(_go)()
        return True, "", ""
    except Exception as exc:  # noqa: BLE001
        text = str(exc).lower()
        if any(s in text for s in ("401", "unauthorized", "api key", "authentication")):
            return False, "provider_auth_failed", "API key rejected."
        if any(s in text for s in ("connection", "timeout", "unreachable", "resolve")):
            return False, "provider_unreachable", "Could not reach the endpoint."
        if any(s in text for s in ("model", "not found", "does not exist")):
            return False, "model_invalid", "Model not accepted by the provider."
        return False, "provider_unreachable", "Could not reach the endpoint."
