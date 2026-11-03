# Web edge fixtures (domain D-00)

Golden recordings of the two public function views in `apps/api/pi_dash/web/`.
Rendered with the project's pinned Django **4.2.30**
(`apps/api/requirements/base.txt:4`); `JsonResponse` separators/encoding
are version-pinned behaviour — recorded, not assumed.

## Goldens

- F-WEB-01: `GET /` → `200`, content-type `application/json`,
  body `F-WEB-01.body` (`{"status": "OK"}`, 16 bytes,
  sha256 `d1940c2d…d49f3f0342`).
  Trace: `apps/api/pi_dash/web/views.py:8-9` (`health_check`),
  `apps/api/pi_dash/web/urls.py:8` (`path("")`),
  mounted at the site root via `apps/api/pi_dash/urls.py:28`.
- F-WEB-02: `GET /robots.txt` → `200`, content-type `text/plain`,
  body `F-WEB-02.body` (`User-agent: *\nDisallow: /`, 25 bytes,
  sha256 `efdb5938…b8efd5dcce`).
  Trace: `apps/api/pi_dash/web/views.py:12-13` (`robots_txt`),
  `apps/api/pi_dash/web/urls.py:8` (`path("robots.txt")`),
  mounted at the site root via `apps/api/pi_dash/urls.py:28`.

Each `*.meta.json` repeats its fixture's method, path, status,
content-type, body sha256 and trace line. `*.body` files hold the exact
response bytes with no trailing newline.

Both goldens agree byte-for-byte with the merged PIDASHCONV-14 contract
suite (`rust-api/contract-tests/web_edge/test_web_edge.py`:
`HEALTH_BODY`, `ROBOTS_BODY`).

Empty by inspection (no fixtures): serializers (none), models (none),
queries (no DB access), permissions/throttles (public views, no auth
classes), tasks (none). `apps.py`/`__init__.py` are Django boilerplate,
not ported.

## Documented quirks (not goldens)

- HEAD responses carry no body (Django strips it; status stays 200).
- Unsafe methods without a CSRF token hit the project
  `CSRF_FAILURE_VIEW` (`apps/api/pi_dash/authentication/views/common.py:38`,
  renders `templates/csrf_failure.html` with status **200**, embedding the
  deployment root URL) — pinned by the PIDASHCONV-14 suite, not by bytes here.
