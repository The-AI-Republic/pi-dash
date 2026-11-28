"""Fail unless this interpreter has production's classic ``html.parser``.

The Django oracle must run on CPython 3.12.10, the interpreter in
``apps/api/Dockerfile.api``. Later 3.12.x builds carry the HTML5 rewrite
of ``html.parser`` (``RCDATA_CONTENT_ELEMENTS``): ``strip_tags`` then keeps
``<textarea>``/``<title>`` markup that production drops, and the
``requires_legacy_parser`` link-crawl tests skip. Every CI job that runs
parser-sensitive suites calls this script before installing anything, so a
pin change fails loudly instead of silently changing the oracle.

Usage: ``python rust-api/contract-tests/_harness/assert_legacy_html_parser.py``
"""

import sys
from html.parser import HTMLParser


def main() -> int:
    version = sys.version.split()[0]
    if hasattr(HTMLParser, "RCDATA_CONTENT_ELEMENTS"):
        print(
            f"Python {version} has the new html.parser, which differs from "
            "production (CPython 3.12.10): page strip_tags oracles would "
            "diverge and the requires_legacy_parser link-crawl tests would "
            "skip. Keep this job on CPython 3.12.10 (ubuntu-24.04).",
            file=sys.stderr,
        )
        return 1
    print(f"Python {version}: legacy html.parser OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
