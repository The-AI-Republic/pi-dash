# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""X-Api-Key authenticated runner endpoints used by the local CLI.

The web UI's session-authenticated delete already lives in
``pi_dash.runner.views.runners.RunnerDetailEndpoint``. The CLI cannot
use that surface (no session cookie), so this thin wrapper exposes the
same delete via the ``/api/v1/`` MachineToken / X-Api-Key spine.

Both surfaces delegate to the same shared service
(``pi_dash.runner.services.runner_delete``) so the cascade-vs-cloud-only
semantics stay in lockstep.
"""

from rest_framework import status
from rest_framework.response import Response

from pi_dash.api.views.base import BaseAPIView
from pi_dash.runner.models import Runner
from pi_dash.runner.services.permissions import can_manage_runner
from pi_dash.runner.services.runner_delete import (
    delete_runner as delete_runner_svc,
    parse_purge_local,
)


class RunnerDeleteEndpoint(BaseAPIView):
    """``DELETE /api/v1/runners/<runner_id>/`` — CLI-friendly cascade delete.

    Authenticates via ``X-Api-Key`` (``APIKeyAuthentication`` from
    :mod:`pi_dash.api.middleware.api_authentication`). Authorizes via
    the same ``can_manage_runner`` predicate as the web UI: the
    caller must be the runner's owner *or* an admin of the runner's
    workspace.

    Accepts the same ``?purge_local=true|false`` query flag as the
    web endpoint; default is ``true`` so a CLI that omits the flag
    still gets the cascade behaviour the operator typed
    ``pidash runner remove`` to invoke.
    """

    def delete(self, request, runner_id):
        runner = Runner.objects.filter(pk=runner_id).first()
        if runner is None:
            return Response(
                {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
            )
        if not can_manage_runner(request.user, runner):
            return Response(
                {"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN
            )
        try:
            purge_local = parse_purge_local(request.query_params)
        except ValueError as exc:
            return Response(
                {"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST
            )
        delete_runner_svc(runner, purge_local=purge_local)
        return Response(status=status.HTTP_204_NO_CONTENT)
