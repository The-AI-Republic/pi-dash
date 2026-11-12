# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GET /api/timezones/: public frozen list, sorted by UTC offset."""

ENTRY_KEYS = {"utc_offset", "gmt_offset", "value", "label"}


def test_timezone_list_shape(user_api):
    res = user_api.get("/api/timezones/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == {"timezones"}
    zones = body["timezones"]
    assert len(zones) > 100
    assert all(set(z.keys()) == ENTRY_KEYS for z in zones)
    # Sorted by UTC offset, then label: UTC-11 comes first.
    assert zones[0]["value"] == "Pacific/Pago_Pago"
    assert zones[0]["utc_offset"] == "UTC-11:00"
    assert zones[0]["gmt_offset"] == "GMT-11:00"
    assert any(z["value"] == "America/New_York" for z in zones)
    assert any(z["value"] == "America/Los_Angeles" for z in zones)


def test_timezone_list_is_public(anon_api):
    # TimezoneEndpoint is AllowAny: no denied-permission case exists here;
    # the denied/tenant-isolation floor is covered by the token suite.
    res = anon_api.get("/api/timezones/")
    assert res.status_code == 200
    assert len(res.json()["timezones"]) > 100
