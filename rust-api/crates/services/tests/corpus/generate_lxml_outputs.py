#!/usr/bin/env python3
"""Generate the expected lxml outputs for the Tier-1 HTML corpus.

Reads inputs.json (a list of {"id", "html"}), runs each through live
``lxml.html`` exactly like Django does
(``html.tostring(html.fromstring(value), encoding="unicode")``, any
exception -> null), and writes expected.json.

Usage (from this directory):
    python3 generate_lxml_outputs.py [inputs.json [expected.json]]

Requires lxml 6.x (the oracle Django runs under). The lxml version is
recorded in the output so a regeneration under a different libxml2 shows
up as a diff instead of a silent behaviour change.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> None:
    inputs_path = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "inputs.json"
    expected_path = Path(sys.argv[2]) if len(sys.argv) > 2 else HERE / "expected.json"
    from lxml import etree, html

    cases = json.loads(inputs_path.read_text(encoding="utf-8"))
    expected = {}
    for case in cases:
        value = case["html"]
        try:
            expected[case["id"]] = html.tostring(html.fromstring(value), encoding="unicode")
        except Exception:
            expected[case["id"]] = None
    payload = {
        "lxml_version": etree.__version__ if hasattr(etree, "__version__") else "unknown",
        "cases": expected,
    }
    try:
        import lxml

        payload["lxml_version"] = getattr(lxml, "__version__", payload["lxml_version"])
    except ImportError:
        pass
    expected_path.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(expected)} cases to {expected_path}")


main()
