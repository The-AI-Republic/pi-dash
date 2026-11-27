# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Owned static assets: email templates + email logos (NEWPIDASH-376).

The Rust binary used to compile its email templates in from the frozen
Django tree (``apps/api/templates/emails/``) and every rendered email
linked logo images at ``{{ current_site }}/static/logos/*`` that nothing
on the Rust stack served. Both now live in the active system:

* ``rust-api/templates/emails/`` — byte-identical copy of the 12 frozen
  templates; the 11 ``include_str!`` embeds and the ``test_email.html``
  unit-test read point here, so the Docker builder needs no ``apps/api``.
* ``apps/web_new/public/static/logos/`` — byte-identical copy of the 5
  frozen logo files; Vite ships ``public/`` at the bundle root, the OSS
  Caddyfiles route ``/static/logos/*`` to the web bundle, and the private
  nginx ``.png`` rule already serves them once the image ships the files.

Static only: no database, no broker, no binary, no daemon. Every case
here reads the checkout and fails — never skips — when the wiring drifts.
"""

import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
RUST_TEMPLATES = REPO_ROOT / "rust-api" / "templates" / "emails"
FROZEN_TEMPLATES = REPO_ROOT / "apps" / "api" / "templates" / "emails"
PUBLIC_LOGOS = REPO_ROOT / "apps" / "web_new" / "public" / "static" / "logos"
FROZEN_LOGOS = REPO_ROOT / "apps" / "api" / "pi_dash" / "static" / "logos"
CADDY_CE = REPO_ROOT / "apps" / "proxy" / "Caddyfile.ce"
CADDY_AIO = REPO_ROOT / "apps" / "proxy" / "Caddyfile.aio.ce"
WEB_NEW_NGINX = REPO_ROOT / "apps" / "web_new" / "nginx" / "nginx.conf"

# Every file the frozen tree ships: the 11 embedded templates plus the
# test-only `test_email.html`.
EXPECTED_TEMPLATES = sorted(
    p.relative_to(FROZEN_TEMPLATES).as_posix()
    for p in FROZEN_TEMPLATES.rglob("*")
    if p.is_file()
)

EXPECTED_LOGOS = {"Logo.png", "github_32px.png", "website_32px.png", "twitter_32px.png", "linkedin_32px.png"}


def test_vendored_templates_match_frozen_tree():
    assert EXPECTED_TEMPLATES, "frozen template tree is empty"
    vendored = sorted(
        p.relative_to(RUST_TEMPLATES).as_posix()
        for p in RUST_TEMPLATES.rglob("*")
        if p.is_file()
    )
    assert vendored == EXPECTED_TEMPLATES, "vendored template set drifted"
    for rel in EXPECTED_TEMPLATES:
        assert (RUST_TEMPLATES / rel).read_bytes() == (FROZEN_TEMPLATES / rel).read_bytes(), (
            f"{rel} differs from the frozen original"
        )


def test_vendored_logos_match_frozen_files():
    actual = {p.name for p in PUBLIC_LOGOS.glob("*") if p.is_file()}
    assert actual == EXPECTED_LOGOS, "vendored logo set drifted"
    for name in EXPECTED_LOGOS:
        assert (PUBLIC_LOGOS / name).read_bytes() == (FROZEN_LOGOS / name).read_bytes(), (
            f"{name} differs from the frozen original"
        )


def test_every_rendered_logo_url_resolves_to_a_vendored_file():
    refs = set()
    for rel in EXPECTED_TEMPLATES:
        text = (RUST_TEMPLATES / rel).read_text()
        refs.update(
            m.group(1)
            for m in re.finditer(r"\{\{\s*current_site\s*\}\}/static/logos/(\S+?\.png)", text)
        )
    assert refs, "no logo references found in templates"
    assert refs <= EXPECTED_LOGOS, f"templates reference unvendored logos: {refs - EXPECTED_LOGOS}"


def test_caddy_ce_routes_logos_to_web_new_before_api_static():
    text = CADDY_CE.read_text()
    logos = "reverse_proxy /static/logos/* web_new:3000"
    api_static = "reverse_proxy /static/* api:8000"
    assert logos in text, "CE Caddyfile must route /static/logos/* to web_new"
    assert text.index(logos) < text.index(api_static), (
        "logos route must precede the /static/* -> api rule (first match wins)"
    )


def test_caddy_aio_serves_logos_from_web_new_bundle():
    text = CADDY_AIO.read_text()
    handle = "handle /static/logos/*"
    assert handle in text, "aio Caddyfile must serve /static/logos/* from the web bundle"
    assert "root * /app/web_new" in text[text.index(handle) : text.index(handle) + 400], (
        "aio logos handle must serve from /app/web_new"
    )
    assert text.index(handle) < text.index("BEGIN MIGRATED ROUTES"), (
        "aio logos handle must sit above the generated migrated-routes block"
    )


def test_web_new_nginx_serves_static_files_from_bundle_root():
    text = WEB_NEW_NGINX.read_text()
    assert "try_files $uri" in text, "web_new nginx must serve $uri (public/ files) from the bundle"
