"""Secure-request CSRF through a TLS proxy (PIDASHCONV-1228).

Production sets ``SECURE_PROXY_SSL_HEADER``, so ``X-Forwarded-Proto:
https`` makes Django's ``request.is_secure()`` true. The prompting CSRF
gate must then use ``https://<host>`` as the same-origin good origin and
run the Referer branch for requests that carry no ``Origin``.

The backend under test must run the production profile (Django
``pi_dash.settings.production``; Rust ``DJANGO_SETTINGS_MODULE`` ending in
``production``). ``CORS_ALLOWED_ORIGINS`` stays empty, so no origin is
trusted.
"""

import urllib.parse


def _host(api):
    return urllib.parse.urlsplit(str(api.base_url)).netloc


def _detail_url(tenant, section_key="implementation"):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/prompt-sections/{section_key}"
    )


def _secure(write, **extra):
    headers = dict(write)
    headers["X-Forwarded-Proto"] = "https"
    headers.update(extra)
    return headers


def test_secure_https_same_origin_passes(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a, Origin=f"https://{_host(api)}"),
        json={"scope": "workspace", "body": "secure same origin"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["body"] == "secure same origin"


def test_secure_delete_https_same_origin_passes(api, tenant_a, write_a):
    url = _detail_url(tenant_a)
    r = api.put(url, headers=write_a,
                json={"scope": "workspace", "body": "delete me"})
    assert r.status_code == 200, r.text
    r = api.delete(
        url + "?scope=workspace",
        headers=_secure(write_a, Origin=f"https://{_host(api)}"),
    )
    assert r.status_code == 204, r.text


def test_secure_preview_https_same_origin_passes(api, tenant_a, issue_a,
                                                 write_a):
    r = api.post(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        "/prompts/coding-task/preview",
        headers=_secure(write_a, Origin=f"https://{_host(api)}"),
        json={"issue_id": str(issue_a)},
    )
    assert r.status_code == 200, r.text


def test_secure_no_referer_is_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a),
        json={"scope": "workspace", "body": "no referer"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {
        "detail": "CSRF Failed: Referer checking failed - no Referer."
    }


def test_secure_http_referer_is_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a, Referer=f"http://{_host(api)}/app/"),
        json={"scope": "workspace", "body": "insecure referer"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {
        "detail": (
            "CSRF Failed: Referer checking failed - Referer is insecure "
            "while host is secure."
        )
    }


def test_secure_cross_site_referer_is_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a, Referer="https://evil.example/x"),
        json={"scope": "workspace", "body": "cross site referer"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {
        "detail": (
            "CSRF Failed: Referer checking failed - https://evil.example/x "
            "does not match any trusted origins."
        )
    }


def test_secure_malformed_referer_is_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a, Referer="https://[::1"),
        json={"scope": "workspace", "body": "malformed referer"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {
        "detail": (
            "CSRF Failed: Referer checking failed - Referer is malformed."
        )
    }


def test_secure_http_origin_is_rejected(api, tenant_a, write_a):
    origin = f"http://{_host(api)}"
    r = api.put(
        _detail_url(tenant_a),
        headers=_secure(write_a, Origin=origin),
        json={"scope": "workspace", "body": "insecure origin"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {
        "detail": (
            "CSRF Failed: Origin checking failed - "
            f"{origin} does not match any trusted origins."
        )
    }
