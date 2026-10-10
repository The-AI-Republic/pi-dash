"""Unit tests for the V-02 matrix + gate helpers. Needs no backend."""

import pytest

from _harness import rust_matrix

SUITES = ["app_issues", "web_edge"]


def test_known_failures_skips_fenced_example(tmp_path):
    """The documented entry format must never parse as a live entry."""
    doc = tmp_path / "KNOWN_RUST_FAILURES.md"
    doc.write_text(
        "# Known Rust failures\n\n"
        "```md\n"
        "- `app_issues`: PIDASHCONV-822 — example\n"
        "```\n\n"
        "## Suites\n\n"
        "(none — the list is empty)\n"
    )
    assert rust_matrix.known_failures(doc) == {}


def test_known_failures_parses_entries_and_rejects_dupes(tmp_path):
    doc = tmp_path / "KNOWN_RUST_FAILURES.md"
    doc.write_text(
        "## Suites\n\n"
        "- `app_issues`: PIDASHCONV-822 — cursor encoding\n"
        "- `web_edge`: PIDASHCONV-823 — robots bytes\n"
    )
    assert rust_matrix.known_failures(doc) == {
        "app_issues": "PIDASHCONV-822",
        "web_edge": "PIDASHCONV-823",
    }
    doc.write_text(
        "## Suites\n\n"
        "- `app_issues`: PIDASHCONV-822 — one\n"
        "- `app_issues`: PIDASHCONV-824 — two\n"
    )
    with pytest.raises(ValueError, match="duplicate entry"):
        rust_matrix.known_failures(doc)


def _verdict(**kw):
    args = {"suite": "app_issues", "django_exit": 0, "rust_exit": 0,
            "suites": SUITES, "known": {}}
    args.update(kw)
    return rust_matrix.gate_verdict(**args)


def test_gate_pass_pass_ok():
    code, msg = _verdict()
    assert code == 0 and "both backends" in msg


def test_gate_unknown_suite_fails():
    code, _ = _verdict(suite="nope")
    assert code == 1


def test_gate_django_failure_fails_even_when_listed():
    code, msg = _verdict(django_exit=1, known={"app_issues": "PIDASHCONV-822"})
    assert code == 1 and "oracle" in msg


def test_gate_unlisted_rust_failure_fails():
    code, msg = _verdict(rust_exit=1)
    assert code == 1 and "not listed" in msg


def test_gate_listed_rust_failure_passes():
    code, msg = _verdict(rust_exit=1, known={"app_issues": "PIDASHCONV-822"})
    assert code == 0 and "as listed" in msg


def test_gate_listed_rust_pass_is_stale():
    code, msg = _verdict(rust_exit=0, known={"app_issues": "PIDASHCONV-822"})
    assert code == 1 and "still listed" in msg


@pytest.mark.parametrize("exit", [2, 3, 4, 5, -1])
def test_gate_non_test_rust_exit_always_fails(exit):
    code, _ = _verdict(rust_exit=exit)
    assert code == 1
    code, _ = _verdict(rust_exit=exit, known={"app_issues": "PIDASHCONV-822"})
    assert code == 1


def test_check_passes_on_real_tree():
    assert rust_matrix.cmd_check() == 0


def test_matrix_lists_every_http_suite():
    suites = rust_matrix.http_suites()
    assert len(suites) == 33
    assert "web_edge" in suites and "ops" not in suites
