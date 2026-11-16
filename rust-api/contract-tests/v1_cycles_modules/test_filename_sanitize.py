"""Contract tests: exact filename sanitizing (PIDASHCONV-694, D-20).

Pins ``MultiPartParser.sanitize_file_name`` byte-for-byte through the live
backends: the full html5 entity table (named, legacy no-semicolon,
longest-prefix fallback, numeric refs with the WHATWG remap + invalid
rules), unescape-before-strip ordering, and CPython ``str.isprintable``
edges. Sanitized names surface in the module ``lead`` / cycle
``owned_by`` 400 echoes (``"<name>" is not a valid UUID.`` with smart
quotes) and the ``members`` echo; on the unknown-key 500 path a ``None``
sanitize skips the part and the request 201s instead.

Bodies are hand-encoded (not httpx ``files=``) so every filename byte is
exact, including quotes, NUL-free controls via entities, and non-UTF-8
transport shapes.
"""

import uuid

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

CYCLES = api.cycles_url(SLUG, PROJ)
MODULES = api.modules_url(SLUG, PROJ)

LQ = "“"
RQ = "”"


def _name(prefix):
    return f"{prefix} {uuid.uuid4().hex[:8]}"


def _multipart(parts, boundary=b"----san694", req_ct=None):
    """Hand-encoded multipart; parts are (name, filename_or_None, data)."""
    body = b""
    for name, filename, data in parts:
        disp = b'Content-Disposition: form-data; name="' + name + b'"'
        if filename is not None:
            disp += b'; filename="' + filename + b'"'
        body += b"--" + boundary + b"\r\n" + disp + b"\r\n\r\n" + data + b"\r\n"
    body += b"--" + boundary + b"--\r\n"
    content_type = req_ct or (
        b"multipart/form-data; boundary=" + boundary
    ).decode("ascii")
    return body, content_type


def _post_module(admin_client, filename, field=b"lead", name=None, req_ct=None):
    name = name or _name("FN")
    body, content_type = _multipart(
        [(b"name", None, name.encode()), (field, filename, b"x")],
        req_ct=req_ct,
    )
    r = admin_client.post(
        MODULES, content=body, headers={"Content-Type": content_type}
    )
    return name, r


def _uuid_echo(sanitized):
    return f"{LQ}{sanitized}{RQ} is not a valid UUID."


class TestLeadFilenameEcho:
    """Module ``lead`` file: the sanitized name echoes in the 400."""

    CASES = [
        # (raw filename bytes, sanitized name in the echo)
        (b"a&amp;b.txt", "a&b.txt"),
        (b"&lt;evil&gt;.txt", "<evil>.txt"),
        (b"&quot;x&quot;.txt", '"x".txt'),
        (b"a&copyb.txt", "a©b.txt"),  # legacy no-semicolon resolves
        (b"a&hellipb.txt", "a&hellipb.txt"),  # non-legacy stays literal
        (b"&notanentity;.txt", "¬anentity;.txt"),  # longest-prefix fallback
        (b"&#x41;&#66;.txt", "AB.txt"),
        (b"&#x80;.txt", "€.txt"),  # WHATWG cp1252 remap
        (b"&#13;.txt", ".txt"),  # CR unescapes, then isprintable drops it
        (b"&#xD800;.txt", "\ufffd.txt"),  # surrogate ref -> U+FFFD
        (b"a&#x1;b.txt", "ab.txt"),  # invalid codepoint -> ""
        (b"..&#x2F;evil.txt", "evil.txt"),  # unescape runs before the strip
        (b"a&#x5C;b.txt", "b.txt"),
        (b"C:some_file.txt", "C:some_file.txt"),  # drive prefix survives
        (b"my file.txt", "my file.txt"),  # ASCII space is printable
        ("a\u00a0b.txt".encode(), "ab.txt"),  # NBSP is not
        ("café.txt".encode(), "café.txt"),  # no mojibake
        ("a\u200eb.txt".encode(), "ab.txt"),  # LEFT-TO-RIGHT MARK (Cf)
        ("a\u200bb.txt".encode(), "ab.txt"),  # ZERO WIDTH SPACE (Cf)
        ("a\U0001F600b.txt".encode(), "a\U0001F600b.txt"),  # astral survives
        ("e\u0301.txt".encode(), "e\u0301.txt"),  # combining mark (Mn) survives
        ("\u0378.txt".encode(), ".txt"),  # unassigned (Cn) drops
        ("\ue000.txt".encode(), ".txt"),  # private use (Co) drops
        ("\ufffd.txt".encode(), "\ufffd.txt"),  # U+FFFD (So) survives
        (b'a"b.txt', 'a"b.txt'),  # quote survives header parsing
        (b"a;b.txt", "a;b.txt"),
    ]

    def test_lead_echo_matrix(self, admin_client):
        for raw, sanitized in self.CASES:
            _, r = _post_module(admin_client, raw)
            assert r.status_code == 400, raw
            assert r.json() == {"lead": [_uuid_echo(sanitized)]}, raw

    def test_dot_names_skip_the_part(self, admin_client):
        # A None sanitize means "no file here": the lead file vanishes and
        # the create 201s with lead null.
        for raw in (b"&#46;&#46;", b".", b".."):
            sent, r = _post_module(admin_client, raw)
            assert r.status_code == 201, raw
            assert r.json()["name"] == sent, raw
            assert r.json()["lead"] is None, raw

    def test_empty_filename_is_a_field(self, admin_client):
        # filename="" is not a file at all (P23): the part body ('x')
        # arrives as the lead value.
        _, r = _post_module(admin_client, b"")
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("x")]}


class TestMembersFilenameEcho:
    def test_members_file_echo(self, admin_client):
        _, r = _post_module(admin_client, "m€.txt".encode(), field=b"members")
        assert r.status_code == 400
        assert r.json() == {"members": {"0": [_uuid_echo("m€.txt")]}}

    def test_members_order_trap(self, admin_client):
        _, r = _post_module(admin_client, b"..&#x2F;evil.txt", field=b"members")
        assert r.status_code == 400
        assert r.json() == {"members": {"0": [_uuid_echo("evil.txt")]}}


class TestCycleOwnedByFilenameEcho:
    def test_owned_by_echo_matrix(self, admin_client):
        for raw, sanitized in [
            ("m€.txt".encode(), "m€.txt"),
            (b"..&#x2F;evil.txt", "evil.txt"),
        ]:
            sent = _name("FN")
            body, content_type = _multipart(
                [
                    (b"name", None, sent.encode()),
                    (b"owned_by", raw, b"x"),
                ]
            )
            r = admin_client.post(
                CYCLES, content=body, headers={"Content-Type": content_type}
            )
            assert r.status_code == 400, raw
            assert r.json() == {"owned_by": [_uuid_echo(sanitized)]}, raw


class TestUnknownKeyFilenameShapes:
    def test_unknown_file_500_exact(self, admin_client):
        # Unknown keys 500 after save through the JSON envelope (pinned
        # byte-for-byte); the entity in the name is irrelevant to it.
        _, r = _post_module(admin_client, b"a&amp;b.txt", field=b"nope")
        assert r.status_code == 500
        assert r.json() == {"error": "Something went wrong please try again later"}

    def test_unknown_none_name_skips_to_201(self, admin_client):
        # A None sanitize skips the part before the unknown-key 500.
        sent, r = _post_module(admin_client, b"..", field=b"nope")
        assert r.status_code == 201
        assert r.json()["name"] == sent


class TestUtf16Filenames:
    """Request ``charset=utf-16`` reinterprets the (valid-UTF-8) filename
    bytes as UTF-16LE: lone surrogates and odd tails become U+FFFD, which
    sanitizing keeps (So). Params and field data are UTF-16LE below so the
    parts still classify."""

    U16 = staticmethod(lambda s: s.encode("utf-16-le"))

    def _post_u16(self, admin_client, filename):
        sent = _name("FN")
        body, _ = _multipart(
            [
                (self.U16("name"), None, self.U16(sent)),
                (self.U16("lead"), filename, b"x"),
            ]
        )
        r = admin_client.post(
            MODULES,
            content=body,
            headers={
                "Content-Type": "multipart/form-data; boundary=----san694; charset=utf-16"
            },
        )
        return sent, r

    def test_utf16_pair(self, admin_client):
        _, r = self._post_u16(admin_client, b"ab")
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("\u6261")]}

    def test_utf16_lone_high_surrogate(self, admin_client):
        _, r = self._post_u16(admin_client, b"a\xd8\x80b")
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("\ufffd\u6280")]}

    def test_utf16_odd_tail(self, admin_client):
        _, r = self._post_u16(admin_client, b"abc")
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("\u6261\ufffd")]}

    def test_utf16_lone_low_surrogate(self, admin_client):
        _, r = self._post_u16(admin_client, b"a\xde\x80b")
        assert r.status_code == 400
        assert r.json() == {"lead": [_uuid_echo("\ufffd\u6280")]}
