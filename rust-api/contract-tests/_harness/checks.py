"""Small shape-assertion helpers shared by every domain suite."""

import re

_DRF_DATETIME_RE = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,6})?(Z|[+-]\d{2}:\d{2})$"
)


def assert_drf_datetime(value, ctx: str = "") -> None:
    """Pin DRF datetime precision: micros exist only as 1-6 digits.

    Django/DRF renders Python datetimes (microsecond resolution): no
    fraction when microsecond == 0, exactly 6 digits otherwise. Any
    9-digit nanos rendering fails here (PIDASHCONV-867).
    """
    assert isinstance(value, str), \
        f"{ctx}: expected datetime string, got {type(value)}"
    assert _DRF_DATETIME_RE.match(value), \
        f"{ctx}: not a DRF microsecond datetime: {value!r}"


def assert_drf_datetimes(obj: dict, keys, ctx: str = "") -> None:
    """Pin every non-null datetime key in ``keys`` to DRF precision."""
    for key in keys:
        value = obj.get(key)
        if value is not None:
            label = f"{ctx}.{key}" if ctx else key
            assert_drf_datetime(value, label)


def require_keys(obj: dict, keys, ctx: str = "") -> None:
    assert isinstance(obj, dict), f"{ctx}: expected object, got {type(obj)}"
    missing = [k for k in keys if k not in obj]
    assert not missing, f"{ctx}: missing keys {missing} in {sorted(obj)}"


def forbid_keys(obj: dict, keys, ctx: str = "") -> None:
    present = [k for k in keys if k in obj]
    assert not present, f"{ctx}: leaked keys {present}"
