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
from pi_dash.runner.models import MachineToken
from pi_dash.runner.services.permissions import is_workspace_member
from pi_dash.runner.services.tokens import hash_token


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
        token_hash = hash_token(token)
        try:
            machine_token = MachineToken.objects.select_related("user", "workspace", "dev_machine").get(
                token_hash=token_hash
            )
        except MachineToken.DoesNotExist:
            raise AuthenticationFailed("Given API token is not valid")
        if machine_token.revoked_at is not None:
            raise AuthenticationFailed("Given API token is not valid")
        if machine_token.dev_machine_id is not None and machine_token.dev_machine.revoked_at is not None:
            raise AuthenticationFailed("Given API token is not valid")
        if not is_workspace_member(machine_token.user, machine_token.workspace_id):
            machine_token.revoke()
            raise AuthenticationFailed("Given API token is not valid")
        MachineToken.objects.filter(pk=machine_token.pk).update(last_used_at=timezone.now())
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
