# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app + spaces credential-auth write flows (PIDASHCONV-823).

Built from the ``auth_session`` contract suite: test_signup.py,
test_signin.py, test_signout.py (form Views, always 302, CSRF-primed),
test_email_check.py, test_change_password.py, test_set_password.py,
test_forgot_password.py, test_reset_password.py (DRF APIViews) and
tokens.py (stdlib reset-token mint).

Browser discipline: every CSRF-enforced POST (all plain-Django form
Views, plus the authenticated DRF posts — stock DRF SessionAuthentication
enforces CSRF when a session user is present) is preceded by a fresh
``GET /auth/get-csrf-token/`` prime, and the POST carries that prime's
cookie + header pair. Login rotates the secret, so a prime is never
reused across a login; each prime has unique capture names.
"""

from auth_session.tokens import mint_reset_token

from .. import effects
from ..session import Request, Session
from ._csrf import csrf as _csrf
from ._csrf import prime as _prime
from .world import PASSWORD

NEW_PASSWORD = "Replay-New-Pass-2!x"
WRONG_PASSWORD = "Wrong-Pass-1!x"

MEMBER_EMAIL = "replay-member@example.com"
GUEST_EMAIL = "replay-guest@example.com"
OUTSIDER_EMAIL = "replay-outsider@example.com"
SIGNUP_EMAIL = "replay-signup@example.com"
SPACE_SIGNUP_EMAIL = "replay-space-signup@example.com"
AUTOSET_EMAIL = "replay-autoset@example.com"
UNKNOWN_EMAIL = "replay-nobody@example.com"


def _seed_extras(conn) -> None:
    """Autoset user for the set-password flow (mirrors world user seed)."""
    from _harness import db as db_helpers
    from _harness import seed as seed_helpers

    db_helpers.create_user(
        conn, "replay-autoset", first_name="Replay", last_name="Autoset",
        email=AUTOSET_EMAIL,
    )
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE users SET password = %s, is_password_autoset = true"
            " WHERE email = %s",
            (seed_helpers.make_password(PASSWORD), AUTOSET_EMAIL),
        )
    conn.commit()


def _setup(world: dict, config) -> dict:
    """Mint password-reset tokens for the two reset users.

    Read from the Django copy (identical to the Rust clone at this
    point): the corpus never signs in as, and never changes the password
    of, either user before its reset request, so the minted tokens
    validate on both backends.
    """
    users = world["users"]
    out_uid, out_token = mint_reset_token(
        config.django_db, config.secret_key, users["outsider"]["id"]
    )
    adm_uid, adm_token = mint_reset_token(
        config.django_db, config.secret_key, users["admin"]["id"]
    )
    return {
        "reset_uid": out_uid,
        "reset_token": out_token,
        "sreset_uid": adm_uid,
        "sreset_token": adm_token,
    }


SESSIONS = (
    Session(
        name="auth_session_lifecycle",
        domain="auth_session",
        source="auth_session/test_signup.py, test_signin.py, test_signout.py,"
        " test_email_check.py, test_change_password.py, test_set_password.py,"
        " test_forgot_password.py, test_reset_password.py, tokens.py",
        auth_users=("member", "guest"),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=("reset_uid", "reset_token", "sreset_uid", "sreset_token"),
        # Views mint password hashes with random salts (sign-up, change,
        # set, reset), random hex usernames + tokens (sign-up), and random
        # profile colors (Profile.background_color default=get_random_color,
        # db/models/user.py:254): the columns still compare in every other
        # session, whose users all come from the cloned snapshot.
        generated_columns={
            "users": ["password", "token", "username"],
            "profiles": ["background_color"],
        },
        # forgot_password carries a freshly minted reset token per
        # backend (seconds apart): each side's token validates against
        # its own user row instead of comparing bytes.
        payload_reset_tokens={
            "pi_dash.bgtasks.forgot_password_task.forgot_password": [
                ("args/2", "args/3"),
            ],
        },
        # Known divergence: email sign-up skips the post_save
        # notification-preferences row (PIDASHCONV-877). Pins the exact
        # normalised row; fails if it stops appearing Django-only.
        known_extra_rows=[
            ("user_notification_preferences", "PIDASHCONV-877", {
                "comment": True,
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "created_by_id": None,
                "deleted_at": None,
                "id": effects.VOLATILE_PLACEHOLDER,
                "issue_completed": True,
                "mention": True,
                "project_id": None,
                "property_change": True,
                "state_change": True,
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "updated_by_id": None,
                "user_id": "{{row:users}}",
                "workspace_id": None,
            }),
            # Spaces sign-up skips the adapter's Profile.objects.create
            # (PIDASHCONV-879). Same exact-row pin discipline.
            ("profiles", "PIDASHCONV-879", {
                "background_color": effects.VOLATILE_PLACEHOLDER,
                "billing_address": None,
                "billing_address_country": "INDIA",
                "company_name": "",
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "goals": {},
                "has_billing_address": False,
                "has_marketing_email_consent": False,
                "id": effects.VOLATILE_PLACEHOLDER,
                "is_app_rail_docked": True,
                "is_mobile_onboarded": False,
                "is_navigation_tour_completed": False,
                "is_onboarded": False,
                "is_smooth_cursor_enabled": False,
                "is_subscribed_to_changelog": False,
                "is_tour_completed": False,
                "language": "en",
                "last_workspace_id": None,
                "mobile_onboarding_step": {"profile_complete": False, "workspace_create": False, "workspace_join": False},
                "mobile_timezone_auto_set": False,
                "notification_view_mode": "full",
                "onboarding_step": {"profile_complete": False, "workspace_create": False, "workspace_invite": False, "workspace_join": False},
                "product_tour": {"cycles": False, "intake": False, "modules": False, "pages": False, "work_items": False},
                "role": None,
                "settings": {},
                "start_of_the_week": 0,
                "theme": {},
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "use_case": None,
                "user_id": "{{row:users}}",
            }),
        ],
        # Known divergence: change/set-password re-login leaks the old
        # session row instead of deleting it like Django login()
        # (PIDASHCONV-878). Pins the exact normalised row; fails if Rust
        # stops leaking (fix landed -> remove the pin).
        known_rust_extra_rows=[
            ("sessions", "PIDASHCONV-878", {
                "user_id": "{{created_user}}",
            }),
        ],
        # Signal-created rows for view-created users (sign-up): the new
        # ids exist on their own side but are capturable only later, so
        # the FKs blank by existence like payload row refs.
        db_cell_refs=[
            ("user_notification_preferences", "user_id", "users", ""),
            ("profiles", "user_id", "users", ""),
        ],
        requests=(
            # -- email-check (DRF, anon JSON, no CSRF) --
            Request(
                "POST", "/auth/email-check/",
                route="/auth/email-check/",
                json={"email": MEMBER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/email-check/",
                route="/auth/email-check/",
                json={},
                auth="anonymous", expect_status=400,
            ),
            # -- sign-up (form View, always 302) --
            _prime("su1"),
            Request(
                "POST", "/auth/sign-up/",
                route="/auth/sign-up/",
                form={"email": SIGNUP_EMAIL, "password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("su1"),
            ),
            _prime("su2"),
            Request(
                "POST", "/auth/sign-up/",
                route="/auth/sign-up/",
                form={"email": MEMBER_EMAIL, "password": PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("su2"),
            ),
            _prime("su3"),
            Request(
                "POST", "/auth/sign-up/",
                route="/auth/sign-up/",
                form={"email": SIGNUP_EMAIL},
                auth="anonymous", expect_status=302, **_csrf("su3"),
            ),
            # -- sign-in (form View, always 302) --
            _prime("si1"),
            Request(
                "POST", "/auth/sign-in/",
                route="/auth/sign-in/",
                form={"email": MEMBER_EMAIL, "password": PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("si1"),
            ),
            _prime("si2"),
            Request(
                "POST", "/auth/sign-in/",
                route="/auth/sign-in/",
                form={"email": MEMBER_EMAIL, "password": WRONG_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("si2"),
            ),
            # -- set-password rejects an already-set password (member is
            # valid only until change-password below invalidates the
            # forged session, so this runs first) --
            _prime("sp3"),
            Request(
                "POST", "/auth/set-password/",
                route="/auth/set-password/",
                json={"password": NEW_PASSWORD},
                auth="member", expect_status=400, **_csrf("sp3"),
            ),
            # -- change-password (DRF, authed JSON + CSRF). Success
            # re-logs-in (common.py), killing the forged session: later
            # member requests use the real si3 cookie --
            _prime("cp1"),
            Request(
                "POST", "/auth/change-password/",
                route="/auth/change-password/",
                json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
                auth="member", expect_status=200, **_csrf("cp1"),
            ),
            _prime("si3"),
            Request(
                "POST", "/auth/sign-in/",
                route="/auth/sign-in/",
                form={"email": MEMBER_EMAIL, "password": NEW_PASSWORD},
                auth="anonymous",
                capture={"member_sess3": "@cookie:session-id"},
                expect_status=302, **_csrf("si3"),
            ),
            _prime("cp2"),
            Request(
                "POST", "/auth/change-password/",
                route="/auth/change-password/",
                json={"old_password": WRONG_PASSWORD, "new_password": NEW_PASSWORD},
                auth="guest", expect_status=400, **_csrf("cp2"),
            ),
            Request(
                "POST", "/auth/change-password/",
                route="/auth/change-password/",
                json={"old_password": "x", "new_password": NEW_PASSWORD},
                auth="anonymous", expect_status=401,
            ),
            # -- set-password (DRF, authed JSON + CSRF; cookie-authed via a
            # real sign-in as the seeded autoset user, no forge needed).
            # Success re-logs-in too, so sp2 uses sp1's fresh cookie --
            _prime("si4"),
            Request(
                "POST", "/auth/sign-in/",
                route="/auth/sign-in/",
                form={"email": AUTOSET_EMAIL, "password": PASSWORD},
                auth="anonymous",
                capture={"autoset_sess": "@cookie:session-id"},
                expect_status=302, **_csrf("si4"),
            ),
            _prime("sp1"),
            Request(
                "POST", "/auth/set-password/",
                route="/auth/set-password/",
                json={"password": NEW_PASSWORD},
                auth="cookie:autoset_sess",
                # The view rotates users.token and returns it: random per
                # backend on both sides, so the capture blanks it.
                capture={
                    "autoset_sess2": "@cookie:session-id",
                    "autoset_token": "token",
                },
                expect_status=200, **_csrf("sp1"),
            ),
            _prime("sp2"),
            Request(
                "POST", "/auth/set-password/",
                route="/auth/set-password/",
                json={"password": NEW_PASSWORD},
                auth="cookie:autoset_sess2", expect_status=400, **_csrf("sp2"),
            ),
            # -- forgot-password (DRF, anon JSON) + reset (form View) --
            Request(
                "POST", "/auth/forgot-password/",
                route="/auth/forgot-password/",
                json={"email": OUTSIDER_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/auth/forgot-password/",
                route="/auth/forgot-password/",
                json={"email": UNKNOWN_EMAIL},
                auth="anonymous", expect_status=400,
            ),
            _prime("rp1"),
            Request(
                "POST", "/auth/reset-password/{reset_uid}/{reset_token}/",
                route="/auth/reset-password/{uidb64}/{token}/",
                form={"password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("rp1"),
            ),
            _prime("rp2"),
            Request(
                "POST", "/auth/reset-password/{reset_uid}/{reset_token}/",
                route="/auth/reset-password/{uidb64}/{token}/",
                form={"password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("rp2"),
            ),
            _prime("rp3"),
            Request(
                "POST", "/auth/reset-password/{reset_uid}/bogus-token/",
                route="/auth/reset-password/{uidb64}/{token}/",
                form={"password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("rp3"),
            ),
            # -- spaces twins --
            Request(
                "POST", "/auth/spaces/email-check/",
                route="/auth/spaces/email-check/",
                json={"email": GUEST_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            _prime("ss1"),
            Request(
                "POST", "/auth/spaces/sign-up/",
                route="/auth/spaces/sign-up/",
                form={"email": SPACE_SIGNUP_EMAIL, "password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("ss1"),
            ),
            _prime("sr1"),
            Request(
                "POST", "/auth/spaces/reset-password/{sreset_uid}/{sreset_token}/",
                route="/auth/spaces/reset-password/{uidb64}/{token}/",
                form={"password": NEW_PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("sr1"),
            ),
            Request(
                "POST", "/auth/spaces/forgot-password/",
                route="/auth/spaces/forgot-password/",
                json={"email": GUEST_EMAIL},
                auth="anonymous", expect_status=200,
            ),
            _prime("ss2"),
            Request(
                "POST", "/auth/spaces/sign-in/",
                route="/auth/spaces/sign-in/",
                form={"email": GUEST_EMAIL, "password": PASSWORD},
                auth="anonymous", expect_status=302, **_csrf("ss2"),
            ),
            _prime("so1"),
            Request(
                "POST", "/auth/spaces/sign-out/",
                route="/auth/spaces/sign-out/",
                auth="guest", expect_status=302, **_csrf("so1"),
            ),
            # -- sign-out kills the real si3 session; the dead key is denied --
            _prime("so2"),
            Request(
                "POST", "/auth/sign-out/",
                route="/auth/sign-out/",
                auth="cookie:member_sess3", expect_status=302, **_csrf("so2"),
            ),
            Request(
                "POST", "/auth/change-password/",
                route="/auth/change-password/",
                json={"old_password": NEW_PASSWORD, "new_password": PASSWORD},
                auth="cookie:member_sess3", expect_status=401,
            ),
        ),
    ),
)
