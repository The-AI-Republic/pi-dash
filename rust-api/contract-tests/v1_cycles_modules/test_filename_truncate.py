"""Contract tests: UploadedFile >255 filename truncation (PIDASHCONV-707, D-20).

Pins ``UploadedFile._set_name`` byte-for-byte through the live backends:
names longer than 255 chars are cut to exactly 255 (char-counted,
extension-preserving via ``os.path.splitext``); 255 and below pass through
untouched. Truncated names surface in the module ``lead`` 400 echo
(``"<name>" is not a valid UUID.`` with smart quotes). Multipart field
values have no cap (filenames only).

Bodies are hand-encoded (not httpx ``files=``) so every filename byte is
exact. Filenames stay under the shared 1024-byte header window on both
backends.
"""

import uuid

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

MODULES = api.modules_url(SLUG, PROJ)

LQ = "“"
RQ = "”"


def _name(prefix):
    return f"{prefix} {uuid.uuid4().hex[:8]}"


def _multipart(parts, boundary=b"----tr707"):
    """Hand-encoded multipart; parts are (name, filename_or_None, data)."""
    body = b""
    for name, filename, data in parts:
        disp = b'Content-Disposition: form-data; name="' + name + b'"'
        if filename is not None:
            disp += b'; filename="' + filename + b'"'
        body += b"--" + boundary + b"\r\n" + disp + b"\r\n\r\n" + data + b"\r\n"
    body += b"--" + boundary + b"--\r\n"
    content_type = (b"multipart/form-data; boundary=" + boundary).decode("ascii")
    return body, content_type


def _post_module(admin_client, filename, field=b"lead"):
    sent = _name("TR")
    body, content_type = _multipart(
        [(b"name", None, sent.encode()), (field, filename, b"x")],
    )
    r = admin_client.post(
        MODULES, content=body, headers={"Content-Type": content_type}
    )
    return sent, r


def _uuid_echo(name):
    return f"{LQ}{name}{RQ} is not a valid UUID."


class TestTruncateMatrix:
    """Long names truncate to exactly 255 chars in the lead echo."""

    CASES = [
        # (raw filename bytes, truncated name in the echo)
        (b"a" * 900 + b".txt", "a" * 251 + ".txt"),
        (b"e" * 300, "e" * 255),
        (b"." + b"b" * 300, "." + "b" * 254),  # leading dot: no extension
        (b".." + b"d" * 300 + b".txt", ".." + "d" * 249 + ".txt"),
        (b"name." + b"f" * 300, "." + "f" * 254),  # tail is the extension
        (b"g" * 300 + b".", "g" * 254 + "."),  # bare trailing dot
        (b"jk." + b"l" * 300, "." + "l" * 254),  # ext itself cut, root gone
        (b"." * 300, "." * 255),  # dots only never split
        (b"h" * 200 + b".mid." + b"i" * 100, "h" * 154 + "." + "i" * 100),
        ("é" * 300 + ".txt", "é" * 251 + ".txt"),
        ("é" * 300, "é" * 255),
        ("\U0001F600" * 100 + "a" * 200, "\U0001F600" * 100 + "a" * 155),
    ]

    def test_truncate_matrix(self, admin_client):
        for raw, truncated in self.CASES:
            raw_bytes = raw.encode() if isinstance(raw, str) else raw
            _, r = _post_module(admin_client, raw_bytes)
            assert r.status_code == 400, raw_bytes[:20]
            assert r.json() == {"lead": [_uuid_echo(truncated)]}, raw_bytes[:20]
            assert len(truncated) == 255, raw_bytes[:20]


class TestUntouchedBoundary:
    """255 and below pass through; 256 is the first cut."""

    CASES = [
        (b"p" * 254, "p" * 254),
        (b"p" * 255, "p" * 255),
        (b"p" * 251 + b".txt", "p" * 251 + ".txt"),  # 255 with extension
        (b"." + b"o" * 254, "." + "o" * 254),  # 255 leading-dot
        ("é" * 200 + ".txt", "é" * 200 + ".txt"),  # 204 chars untouched
        ("\U0001F600" * 200, "\U0001F600" * 200),  # 200 astral chars untouched
        (b"q" * 252 + b".txt", "q" * 251 + ".txt"),  # 256 cuts to 255
    ]

    def test_untouched_boundary(self, admin_client):
        for raw, expected in self.CASES:
            raw_bytes = raw.encode() if isinstance(raw, str) else raw
            _, r = _post_module(admin_client, raw_bytes)
            assert r.status_code == 400, raw_bytes[:20]
            assert r.json() == {"lead": [_uuid_echo(expected)]}, raw_bytes[:20]


class TestFieldValuesHaveNoCap:
    def test_long_field_value_echoes_whole(self, admin_client):
        # Filenames truncate; field values do not: a 900-char lead value
        # echoes back all 900 chars.
        sent = _name("TR")
        body, content_type = _multipart([(b"name", None, sent.encode()), (b"lead", None, b"v" * 900)])
        r = admin_client.post(
            MODULES, content=body, headers={"Content-Type": content_type}
        )
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("v" * 900)]}
