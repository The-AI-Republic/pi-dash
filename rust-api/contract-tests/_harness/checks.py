"""Small shape-assertion helpers shared by every domain suite."""


def require_keys(obj: dict, keys, ctx: str = "") -> None:
    assert isinstance(obj, dict), f"{ctx}: expected object, got {type(obj)}"
    missing = [k for k in keys if k not in obj]
    assert not missing, f"{ctx}: missing keys {missing} in {sorted(obj)}"


def forbid_keys(obj: dict, keys, ctx: str = "") -> None:
    present = [k for k in keys if k in obj]
    assert not present, f"{ctx}: leaked keys {present}"
