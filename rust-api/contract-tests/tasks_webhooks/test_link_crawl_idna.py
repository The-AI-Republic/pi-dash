"""PIDASHCONV-1672: link-crawler IDNA oracle pins.

Pins the Django-observed DNS behavior the Rust SSRF guard must
reproduce: CPython IDNA-encodes a ``str`` hostname before
``socket.getaddrinfo``
(``apps/api/pi_dash/bgtasks/work_item_link_task.py:55``), so
``münchen.de`` resolves while the same name as raw UTF-8 bytes fails
with ``gaierror(-2)`` — the bytes Rust's ``to_socket_addrs`` hands to
libc when the host is not encoded first.

No Django import, no worker, no external DNS: the ``idna`` codec and
the hosts-file ``localhost`` mapping are the only dependencies beyond
pytest and ``socket``. The ``münchen.de`` pin asserts the codec
mapping only; real DNS plus the end-to-end crawl ride in the live
twin tests (``test_crawl_link_resolves_idna_hostname`` in
``test_webhook_tasks.py``, ``test_rust_crawl_link_resolves_idna_hostname``
in ``test_rust_replay.py``).

The Rust side of this contract is pinned by unit tests in
``crates/jobs/src/tasks_webhooks/link_crawl.rs`` asserting the same
mapping through ``live_resolve`` with the fullwidth ``localhost``
below (they fail on pre-fix rust-dev).
"""

import socket


def test_punycode_mapping_matches_idna_codec():
    assert "münchen.de".encode("idna") == b"xn--mnchen-3ya.de"


def test_str_host_is_idna_encoded_before_resolution():
    """A non-ASCII ``str`` host resolves through its ASCII form.

    Fullwidth ``localhost`` folds to ``localhost`` and answers from
    the hosts file, so this needs no external DNS (verified on
    CPython 3.12.3: ``['127.0.0.1']``).
    """
    assert "ｌｏｃａｌｈｏｓｔ".encode("idna") == b"localhost"
    addrs = socket.getaddrinfo("ｌｏｃａｌｈｏｓｔ", None)
    assert addrs, "fullwidth localhost did not resolve"
    assert "127.0.0.1" in {addr[4][0] for addr in addrs}


def test_raw_utf8_bytes_fail_dns():
    """The unencoded bytes Rust hands to libc do not resolve.

    This is the pre-fix Rust behavior: ``(hostname, 0)``
    ``to_socket_addrs()`` on the raw Unicode host fails DNS, so the
    guard reports "Hostname could not be resolved".
    """
    try:
        socket.getaddrinfo("münchen.de".encode(), None)
    except socket.gaierror:
        return
    raise AssertionError("raw UTF-8 host unexpectedly resolved")
