# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: magic-link auth write flows (PIDASHCONV-823).

Built from the ``auth_magic`` contract suite: test_magic_generate.py
(DRF JSON, anonymous), test_magic_sign_in.py, test_magic_sign_up.py and
test_space_magic.py (form Views, always 302, CSRF-primed).

The 6-digit codes live only in Redis and the generate response never
reveals them, so ``setup`` primes the same codes on both backends'
cache Redises (``magic_<email>`` rows shaped exactly like the suite's
``write_magic``) and returns them as template context.
"""

from ..session import Request, Session
from ._csrf import csrf, prime

MEMBER_EMAIL = "replay-member@example.com"
GUEST_EMAIL = "replay-guest@example.com"
OUTSIDER_EMAIL = "replay-outsider@example.com"
UNKNOWN_EMAIL = "replay-nobody@example.com"
GEN_EMAIL = "replay-magic-gen@example.com"
GEN_MAX_EMAIL = "replay-magic-max@example.com"
SIGNUP_EMAIL = "replay-magic-signup@example.com"
SPACE_SIGNUP_EMAIL = "replay-space-magic-signup@example.com"

CODES = {
    "code_in": "111111",
    "code_up": "222222",
    "code_sp_in": "333333",
    "code_sp_up": "444444",
}
CODE_EMAILS = {
    "code_in": MEMBER_EMAIL,
    "code_up": SIGNUP_EMAIL,
    "code_sp_in": GUEST_EMAIL,
    "code_sp_up": SPACE_SIGNUP_EMAIL,
}


def _setup(world: dict, config) -> dict:
    """Prime the same magic codes on both backends' cache Redises."""
    import json

    import redis as redis_lib

    for url in (config.django_redis_url, config.rust_redis_url):
        client = redis_lib.Redis.from_url(url, decode_responses=True)
        try:
            for name, code in CODES.items():
                client.set(
                    f"magic_{CODE_EMAILS[name]}",
                    json.dumps(
                        {"current_attempt": 0, "email": CODE_EMAILS[name], "token": code}
                    ),
                )
        finally:
            client.close()
    return dict(CODES)


SESSIONS = (
    Session(
        name="auth_magic_lifecycle",
        domain="auth_magic",
        source="auth_magic/test_magic_generate.py, test_magic_sign_in.py,"
        " test_magic_sign_up.py, test_space_magic.py",
        auth_users=(),
        setup=_setup,
        setup_keys=tuple(CODES),
        # Views mint password hashes/usernames/tokens (sign-up) and
        # random profile colors, exactly like the credential flows.
        generated_columns={
            "users": ["password", "token", "username"],
            "profiles": ["background_color"],
        },
        # magic_link carries the freshly minted 6-digit code per
        # backend: each side's code must equal its own Redis row.
        payload_magic_codes={
            "pi_dash.bgtasks.magic_link_code_task.magic_link": [
                ("args/0", "args/2"),
            ],
        },
        db_cell_refs=[
            ("user_notification_preferences", "user_id", "users", ""),
            ("profiles", "user_id", "users", ""),
        ],
        requests=(
            # -- magic-generate (DRF, anon JSON, no CSRF) --
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            # Max-attempts arc: four mints, then the 400.
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_MAX_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_MAX_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_MAX_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_MAX_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": GEN_MAX_EMAIL},
                auth="anonymous", expect_status=400,
            ),
            # Existing-user arc (outsider: member/guest codes stay primed
            # for the sign-in requests below): four mints, then the 400.
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/magic-generate/",
                route="/auth/magic-generate/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=400,
            ),
            # -- magic-sign-in (form View, always 302) --
            prime("mi1"),
            Request(
                "POST", "/auth/magic-sign-in/",
                route="/auth/magic-sign-in/",
                form={"email": MEMBER_EMAIL, "code": "000000"},
                auth="anonymous", expect_status=302, **csrf("mi1"),
            ),
            prime("mi2"),
            Request(
                "POST", "/auth/magic-sign-in/",
                route="/auth/magic-sign-in/",
                form={"email": MEMBER_EMAIL, "code": "{code_in}"},
                auth="anonymous", expect_status=302, **csrf("mi2"),
            ),
            prime("mi3"),
            Request(
                "POST", "/auth/magic-sign-in/",
                route="/auth/magic-sign-in/",
                form={"email": UNKNOWN_EMAIL, "code": "123456"},
                auth="anonymous", expect_status=302, **csrf("mi3"),
            ),
            prime("mi4"),
            Request(
                "POST", "/auth/magic-sign-in/",
                route="/auth/magic-sign-in/",
                form={"email": MEMBER_EMAIL},
                auth="anonymous", expect_status=302, **csrf("mi4"),
            ),
            # -- magic-sign-up (form View, always 302) --
            prime("mu1"),
            Request(
                "POST", "/auth/magic-sign-up/",
                route="/auth/magic-sign-up/",
                form={"email": SIGNUP_EMAIL, "code": "{code_up}"},
                auth="anonymous", expect_status=302, **csrf("mu1"),
            ),
            prime("mu2"),
            Request(
                "POST", "/auth/magic-sign-up/",
                route="/auth/magic-sign-up/",
                form={"email": MEMBER_EMAIL, "code": "123456"},
                auth="anonymous", expect_status=302, **csrf("mu2"),
            ),
            prime("mu3"),
            Request(
                "POST", "/auth/magic-sign-up/",
                route="/auth/magic-sign-up/",
                form={},
                auth="anonymous", expect_status=302, **csrf("mu3"),
            ),
            # -- spaces twins --
            Request(
                "POST", "/auth/spaces/magic-generate/",
                route="/auth/spaces/magic-generate/",
                json={"email": GEN_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            prime("ms1"),
            Request(
                "POST", "/auth/spaces/magic-sign-in/",
                route="/auth/spaces/magic-sign-in/",
                form={"email": GUEST_EMAIL, "code": "{code_sp_in}"},
                auth="anonymous", expect_status=302, **csrf("ms1"),
            ),
            prime("ms2"),
            Request(
                "POST", "/auth/spaces/magic-sign-up/",
                route="/auth/spaces/magic-sign-up/",
                form={"email": SPACE_SIGNUP_EMAIL, "code": "{code_sp_up}"},
                auth="anonymous", expect_status=302, **csrf("ms2"),
            ),
            prime("ms3"),
            Request(
                "POST", "/auth/spaces/magic-sign-up/",
                route="/auth/spaces/magic-sign-up/",
                form={"email": SPACE_SIGNUP_EMAIL, "code": "{code_sp_up}"},
                auth="anonymous", expect_status=302, **csrf("ms3"),
            ),
        ),
    ),
)
