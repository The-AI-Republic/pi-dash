# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Third party imports
from rest_framework import status
from rest_framework.response import Response
from drf_spectacular.utils import OpenApiResponse

# Module imports
from apple_pi_dash.api.serializers import UserLiteSerializer
from apple_pi_dash.api.views.base import BaseAPIView
from apple_pi_dash.db.models import User
from apple_pi_dash.utils.openapi.decorators import user_docs
from apple_pi_dash.utils.openapi import USER_EXAMPLE


class UserEndpoint(BaseAPIView):
    serializer_class = UserLiteSerializer
    model = User

    @user_docs(
        operation_id="get_current_user",
        summary="Get current user",
        description="Retrieve the authenticated user's profile information including basic details.",
        responses={
            200: OpenApiResponse(
                description="Current user profile",
                response=UserLiteSerializer,
                examples=[USER_EXAMPLE],
            ),
        },
    )
    def get(self, request):
        """Get current user

        Retrieve the authenticated user's profile information including basic details.
        Returns user data based on the current authentication context.
        """
        serializer = UserLiteSerializer(request.user)
        return Response(serializer.data, status=status.HTTP_200_OK)
