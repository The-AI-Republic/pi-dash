"""Unit tests for the DIFF per-backend expectation marker. Needs no backend."""

import pytest

from _harness import diffs
from _harness.diffs import expect


def test_unset_means_django(monkeypatch):
    monkeypatch.delenv(diffs.BACKEND_ENV_VAR, raising=False)
    assert diffs.current_backend() == "django"
    assert diffs.is_django() and not diffs.is_rust()
    assert expect("DIFF-4", django=500, rust=302) == 500


def test_blank_means_django(monkeypatch):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, "  ")
    assert diffs.current_backend() == "django"
    assert expect("DIFF-4", django=500, rust=302) == 500


def test_explicit_django_selects_django_arm(monkeypatch):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, "django")
    assert diffs.current_backend() == "django"
    assert expect("DIFF-4", django=500, rust=302) == 500


def test_rust_selects_rust_arm(monkeypatch):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, "rust")
    assert diffs.current_backend() == "rust"
    assert diffs.is_rust() and not diffs.is_django()
    assert expect("DIFF-4", django=500, rust=302) == 302


@pytest.mark.parametrize("bad", ["postgres", "Django", "RUST", "both", "1"])
def test_unknown_backend_fails_loudly(monkeypatch, bad):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, bad)
    with pytest.raises(RuntimeError, match="PIDASH_CONTRACT_BACKEND"):
        diffs.current_backend()
    with pytest.raises(RuntimeError, match="PIDASH_CONTRACT_BACKEND"):
        expect("DIFF-4", django=500, rust=302)


@pytest.mark.parametrize("value, arm", [("rust ", "rust"), (" django", "django")])
def test_surrounding_whitespace_is_tolerated(monkeypatch, value, arm):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, value)
    assert diffs.current_backend() == arm


@pytest.mark.parametrize(
    "bad", ["", "diff-4", "DIFF-", "DIFF-x", "DIFF-4 ", " DIFF-4", "PB-1044", "4", None, 4]
)
def test_invalid_diff_id_fails_loudly(monkeypatch, bad):
    monkeypatch.delenv(diffs.BACKEND_ENV_VAR, raising=False)
    with pytest.raises(ValueError, match="DIFF-<n>"):
        expect(bad, django=500, rust=302)


def test_arms_hold_any_type(monkeypatch):
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, "rust")
    assert expect("DIFF-12", django={"code": 5125}, rust=None) is None
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, "django")
    assert expect("DIFF-12", django={"code": 5125}, rust=None) == {"code": 5125}
    assert expect("DIFF-12", django="a", rust="b") == "a"


@pytest.mark.parametrize("backend, pinned", [("django", 500), ("rust", 302)])
def test_drift_from_either_arm_still_fails(monkeypatch, backend, pinned):
    """A DIFF case must fail when its backend drifts from the pinned arm."""
    monkeypatch.setenv(diffs.BACKEND_ENV_VAR, backend)

    def contract_case(actual_status):
        assert actual_status == expect("DIFF-4", django=500, rust=302)

    contract_case(pinned)  # pinned behaviour passes
    with pytest.raises(AssertionError):
        contract_case(200 if pinned != 200 else 201)  # drifted behaviour fails
