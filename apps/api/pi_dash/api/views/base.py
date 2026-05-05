# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
import uuid
import zoneinfo
import logging

# Django imports
from django.conf import settings
from django.core.exceptions import ObjectDoesNotExist, ValidationError
from django.db import IntegrityError
from django.urls import resolve
from django.utils import timezone

# Third party imports
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.viewsets import ModelViewSet
from rest_framework.exceptions import APIException
from rest_framework.generics import GenericAPIView

# Module imports
from pi_dash.db.models.api import APIToken
from pi_dash.api.middleware.api_authentication import APIKeyAuthentication
from pi_dash.api.rate_limit import ApiKeyRateThrottle, ServiceTokenRateThrottle
from pi_dash.utils.exception_logger import log_exception
from pi_dash.utils.paginator import BasePaginator
from pi_dash.utils.core.mixins import ReadReplicaControlMixin


logger = logging.getLogger("pi_dash.api")


class TimezoneMixin:
    """
    This enables timezone conversion according
    to the user set timezone
    """

    def initial(self, request, *args, **kwargs):
        super().initial(request, *args, **kwargs)
        if request.user.is_authenticated:
            timezone.activate(zoneinfo.ZoneInfo(request.user.user_timezone))
        else:
            timezone.deactivate()


def _rewrite_project_kwarg(view, kwargs):
    # Rewrite a slug-or-UUID `project_id` (or `pk`, for the project-detail
    # routes) into the canonical project UUID *before* DRF runs permission
    # checks. Project permission classes filter `ProjectMember` by
    # `project_id=view.project_id` and would silently 403 on a slug otherwise.
    # The lookup is keyed on workspace slug, which we read from the URL kwarg
    # `slug` because the `view.workspace_slug` property already does the same.
    from pi_dash.db.models.project import Project

    # Skip resolution for unauthenticated requests so anonymous callers can't
    # probe slug existence via 404 (slug missing) vs 401 (auth missing).
    # Accessing `request.user` triggers DRF's lazy authentication; the
    # subsequent `super().initial()` -> `check_permissions` will still 401
    # the unauthenticated case before any view code runs.
    if not view.request.user.is_authenticated:
        return

    workspace_slug = kwargs.get("slug")
    if not workspace_slug:
        return

    target_key = None
    if "project_id" in kwargs and kwargs["project_id"] is not None:
        target_key = "project_id"
    elif kwargs.get("pk") is not None and resolve(view.request.path_info).url_name == "project":
        # ProjectDetailAPIEndpoint is registered under url name "project" with
        # the project itself bound to `<str:pk>`. Resolve here so permissions
        # and the view body see a UUID. Mirrors the same `resolve(...)` check
        # used by the existing `BaseAPIView.project_id` property.
        target_key = "pk"
    if target_key is None:
        return

    raw = kwargs[target_key]
    try:
        uuid.UUID(str(raw))
        # Already a UUID — accept it as-is. We deliberately do NOT verify the
        # project exists at this point: the existing view code already does
        # `Project.objects.get(pk=...)` or filters by FK and will 404/403 the
        # same as before. Verifying here would add a query for every request.
        return
    except (ValueError, AttributeError, TypeError):
        pass

    project = Project.resolve(workspace_slug, raw)
    kwargs[target_key] = str(project.pk)
    view.kwargs[target_key] = str(project.pk)


class BaseAPIView(TimezoneMixin, GenericAPIView, ReadReplicaControlMixin, BasePaginator):
    authentication_classes = [APIKeyAuthentication]

    permission_classes = [IsAuthenticated]

    use_read_replica = False

    def initial(self, request, *args, **kwargs):
        # Run BEFORE super().initial() so that DRF's check_permissions sees
        # the rewritten UUID instead of a slug.
        _rewrite_project_kwarg(self, kwargs)
        super().initial(request, *args, **kwargs)

    def filter_queryset(self, queryset):
        for backend in list(self.filter_backends):
            queryset = backend().filter_queryset(self.request, queryset, self)
        return queryset

    def get_throttles(self):
        throttle_classes = []
        api_key = self.request.headers.get("X-Api-Key")

        if api_key:
            service_token = APIToken.objects.filter(token=api_key, is_service=True).first()

            if service_token:
                throttle_classes.append(ServiceTokenRateThrottle())
                return throttle_classes

        throttle_classes.append(ApiKeyRateThrottle())

        return throttle_classes

    def handle_exception(self, exc):
        """
        Handle any exception that occurs, by returning an appropriate response,
        or re-raising the error.
        """
        try:
            response = super().handle_exception(exc)
            return response
        except Exception as e:
            if isinstance(e, IntegrityError):
                return Response(
                    {"error": "The payload is not valid"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            if isinstance(e, ValidationError):
                return Response(
                    {"error": "Please provide valid detail"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            if isinstance(e, ObjectDoesNotExist):
                return Response(
                    {"error": "The requested resource does not exist."},
                    status=status.HTTP_404_NOT_FOUND,
                )

            if isinstance(e, KeyError):
                return Response(
                    {"error": "The required key does not exist."},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            log_exception(e)
            return Response(
                {"error": "Something went wrong please try again later"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR,
            )

    def dispatch(self, request, *args, **kwargs):
        try:
            response = super().dispatch(request, *args, **kwargs)
            if settings.DEBUG:
                from django.db import connection

                print(f"{request.method} - {request.get_full_path()} of Queries: {len(connection.queries)}")
            return response
        except Exception as exc:
            response = self.handle_exception(exc)
            return exc

    def finalize_response(self, request, response, *args, **kwargs):
        # Call super to get the default response
        response = super().finalize_response(request, response, *args, **kwargs)

        # Add custom headers if they exist in the request META
        ratelimit_remaining = request.META.get("X-RateLimit-Remaining")
        if ratelimit_remaining is not None:
            response["X-RateLimit-Remaining"] = ratelimit_remaining

        ratelimit_reset = request.META.get("X-RateLimit-Reset")
        if ratelimit_reset is not None:
            response["X-RateLimit-Reset"] = ratelimit_reset

        return response

    @property
    def workspace_slug(self):
        return self.kwargs.get("slug", None)

    @property
    def project_id(self):
        project_id = self.kwargs.get("project_id", None)
        if project_id:
            return project_id

        if resolve(self.request.path_info).url_name == "project":
            return self.kwargs.get("pk", None)

    @property
    def fields(self):
        fields = [field for field in self.request.GET.get("fields", "").split(",") if field]
        return fields if fields else None

    @property
    def expand(self):
        expand = [expand for expand in self.request.GET.get("expand", "").split(",") if expand]
        return expand if expand else None


class BaseViewSet(TimezoneMixin, ReadReplicaControlMixin, ModelViewSet, BasePaginator):
    model = None

    authentication_classes = [APIKeyAuthentication]
    permission_classes = [
        IsAuthenticated,
    ]
    use_read_replica = False

    def initial(self, request, *args, **kwargs):
        _rewrite_project_kwarg(self, kwargs)
        super().initial(request, *args, **kwargs)

    def get_queryset(self):
        try:
            return self.model.objects.all()
        except Exception as e:
            log_exception(e)
            raise APIException("Please check the view", status.HTTP_400_BAD_REQUEST)

    def handle_exception(self, exc):
        """
        Handle any exception that occurs, by returning an appropriate response,
        or re-raising the error.
        """
        try:
            response = super().handle_exception(exc)
            return response
        except Exception as e:
            if isinstance(e, IntegrityError):
                log_exception(e)
                return Response(
                    {"error": "The payload is not valid"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            if isinstance(e, ValidationError):
                logger.warning(
                    "Validation Error",
                    extra={
                        "error_code": "VALIDATION_ERROR",
                        "error_message": str(e),
                    },
                )
                return Response(
                    {"error": "Please provide valid detail"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            if isinstance(e, ObjectDoesNotExist):
                logger.warning(
                    "Object Does Not Exist",
                    extra={
                        "error_code": "OBJECT_DOES_NOT_EXIST",
                        "error_message": str(e),
                    },
                )
                return Response(
                    {"error": "The required object does not exist."},
                    status=status.HTTP_404_NOT_FOUND,
                )

            if isinstance(e, KeyError):
                logger.error(
                    "Key Error",
                    extra={
                        "error_code": "KEY_ERROR",
                        "error_message": str(e),
                    },
                )
                return Response(
                    {"error": "The required key does not exist."},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            log_exception(e)
            return Response(
                {"error": "Something went wrong please try again later"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR,
            )

    def dispatch(self, request, *args, **kwargs):
        try:
            response = super().dispatch(request, *args, **kwargs)

            if settings.DEBUG:
                from django.db import connection

                print(f"{request.method} - {request.get_full_path()} of Queries: {len(connection.queries)}")

            return response
        except Exception as exc:
            response = self.handle_exception(exc)
            return response

    @property
    def workspace_slug(self):
        return self.kwargs.get("slug", None)

    @property
    def project_id(self):
        project_id = self.kwargs.get("project_id", None)
        if project_id:
            return project_id

        if resolve(self.request.path_info).url_name == "project":
            return self.kwargs.get("pk", None)

    @property
    def fields(self):
        fields = [field for field in self.request.GET.get("fields", "").split(",") if field]
        return fields if fields else None

    @property
    def expand(self):
        expand = [expand for expand in self.request.GET.get("expand", "").split(",") if expand]
        return expand if expand else None
