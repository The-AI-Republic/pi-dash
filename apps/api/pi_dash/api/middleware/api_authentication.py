# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Django imports
from django.utils import timezone
from django.db.models import Q

# Third party imports
from rest_framework import authentication
from rest_framework.exceptions import AuthenticationFailed

# Module imports
from pi_dash.db.models import APIToken


class APIKeyAuthentication(authentication.BaseAuthentication):
    """
    Authentication with an API Key
    """

    www_authenticate_realm = "api"
    media_type = "application/json"
    auth_header_name = "X-Api-Key"

    def get_api_token(self, request):
        return request.headers.get(self.auth_header_name)

    def validate_api_token(self, token):
        try:
            api_token = APIToken.objects.get(
                Q(Q(expired_at__gt=timezone.now()) | Q(expired_at__isnull=True)),
                token=token,
                is_active=True,
            )
        except APIToken.DoesNotExist:
            raise AuthenticationFailed("Given API token is not valid")

        # save api token last used
        api_token.last_used = timezone.now()
        api_token.save(update_fields=["last_used"])
        return (api_token.user, api_token.token)

    def validate_machine_token(self, request, token):
        # Same checks as the runner-plane auth (revoked, 60-day idle expiry,
        # revoked machine, membership), renewing the expiry on use; this
        # surface keeps its generic message.
        from pi_dash.runner.authentication import resolve_machine_token

        try:
            machine_token = resolve_machine_token(token)
        except AuthenticationFailed:
            raise AuthenticationFailed("Given API token is not valid")
        request.auth_machine_token = machine_token
        return (machine_token.user, token)

    def authenticate(self, request):
        token = self.get_api_token(request=request)
        if not token:
            return None

        if token.startswith("mt_"):
            user, token = self.validate_machine_token(request, token)
        else:
            # Validate the API token
            user, token = self.validate_api_token(token)
        return user, token
