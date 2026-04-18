# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import os

from channels.routing import ProtocolTypeRouter, URLRouter
from django.core.asgi import get_asgi_application

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "pi_dash.settings.production")
# Initialize Django ASGI application early to ensure the AppRegistry
# is populated before importing code that may import ORM models.
django_asgi_app = get_asgi_application()

from pi_dash.runner.routing import (  # noqa: E402
    websocket_urlpatterns as runner_ws_urls,
)

application = ProtocolTypeRouter(
    {
        "http": django_asgi_app,
        "websocket": URLRouter(runner_ws_urls),
    }
)
