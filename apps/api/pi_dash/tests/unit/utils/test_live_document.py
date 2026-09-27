# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for :func:`pi_dash.utils.live_document.convert_document`.

The contract that matters: a conversion either yields all three stored
formats or raises — it never quietly returns nothing, because a caller that
saved HTML alone would have its write discarded by the editor.
"""

import base64
from unittest import mock

import pytest
import requests

from pi_dash.utils.live_document import LiveConversionError, convert_document

BINARY = b"\x01\x02\x03\x04state"


def _response(status_code=200, payload=None):
    response = mock.Mock(status_code=status_code)
    response.json.return_value = payload
    return response


@pytest.fixture
def live_url(settings):
    settings.LIVE_URL = "http://live.test/live/"


@pytest.mark.unit
class TestConvertDocument:
    def test_returns_all_three_formats(self, live_url):
        payload = {
            "description_html": "<p class='x'>hi</p>",
            "description_json": {"type": "doc"},
            "description_binary": base64.b64encode(BINARY).decode(),
        }
        with mock.patch("pi_dash.utils.live_document.requests.post", return_value=_response(payload=payload)) as post:
            document = convert_document("<p>hi</p>")

        assert document.description_html == "<p class='x'>hi</p>"
        assert document.description_json == {"type": "doc"}
        assert document.description_binary == BINARY
        assert post.call_args.args[0] == "http://live.test/live/convert-document/"
        assert post.call_args.kwargs["json"] == {"description_html": "<p>hi</p>", "variant": "document"}
        assert post.call_args.kwargs["timeout"]

    def test_sends_base_binary_and_title(self, live_url):
        payload = {"description_json": {}, "description_binary": base64.b64encode(BINARY).decode()}
        with mock.patch("pi_dash.utils.live_document.requests.post", return_value=_response(payload=payload)) as post:
            document = convert_document("<p>hi</p>", base_binary=b"\x00base", title="Name")

        sent = post.call_args.kwargs["json"]
        assert base64.b64decode(sent["description_binary"]) == b"\x00base"
        assert sent["title"] == "Name"
        # An older live server that does not echo HTML back: the sent HTML stands.
        assert document.description_html == "<p>hi</p>"

    def test_unset_live_url_raises(self, settings):
        settings.LIVE_URL = None

        with pytest.raises(LiveConversionError, match="LIVE_URL"):
            convert_document("<p>hi</p>")

    def test_unreachable_live_server_raises(self, live_url):
        with mock.patch("pi_dash.utils.live_document.requests.post", side_effect=requests.ConnectionError()):
            with pytest.raises(LiveConversionError, match="unreachable"):
                convert_document("<p>hi</p>")

    def test_error_status_raises(self, live_url):
        with mock.patch("pi_dash.utils.live_document.requests.post", return_value=_response(status_code=400)):
            with pytest.raises(LiveConversionError, match="HTTP 400"):
                convert_document("<p>hi</p>")

    @pytest.mark.parametrize(
        "payload",
        [
            {"description_json": {}},
            {"description_binary": "not base64!!"},
            {"description_binary": ""},
        ],
    )
    def test_unusable_document_raises(self, live_url, payload):
        with mock.patch("pi_dash.utils.live_document.requests.post", return_value=_response(payload=payload)):
            with pytest.raises(LiveConversionError):
                convert_document("<p>hi</p>")
