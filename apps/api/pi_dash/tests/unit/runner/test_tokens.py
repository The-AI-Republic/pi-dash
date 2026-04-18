# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.utils import timezone

from pi_dash.runner.services import tokens


@pytest.mark.unit
def test_hash_is_deterministic():
    assert tokens.hash_token("abc") == tokens.hash_token("abc")


@pytest.mark.unit
def test_hash_distinguishes_inputs():
    assert tokens.hash_token("a") != tokens.hash_token("b")


@pytest.mark.unit
def test_mint_registration_token_properties():
    minted = tokens.mint_registration_token()
    assert minted.raw.startswith("apd_reg_")
    assert minted.hashed == tokens.hash_token(minted.raw)
    assert minted.expires_at > timezone.now()


@pytest.mark.unit
def test_mint_runner_secret_properties():
    minted = tokens.mint_runner_secret()
    assert minted.raw.startswith("apd_rs_")
    assert minted.hashed == tokens.hash_token(minted.raw)
    assert len(minted.fingerprint) == 12
