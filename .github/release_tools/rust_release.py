"""Validate the independent Rust platform release tag and emit CI metadata."""

import json
import os
import re
from pathlib import Path


PREFIX = "rust-platform-"
NUMBER = r"(?:0|[1-9][0-9]*)"
IDENTIFIER = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = re.compile(
    rf"v{NUMBER}\.{NUMBER}\.{NUMBER}(?:-(?P<prerelease>{IDENTIFIER}(?:\.{IDENTIFIER})*))?"
)


def metadata(tag, release_prerelease=False):
    version = tag.removeprefix(PREFIX)
    match = VERSION.fullmatch(version)
    # Leave room for architecture + commit staging suffixes in Docker tags.
    if not tag.startswith(PREFIX) or match is None or len(version) > 70:
        raise ValueError("Expected rust-platform-vX.Y.Z or rust-platform-vX.Y.Z-rc.1 (max version length 70)")
    return {
        "tag": tag,
        "version": version,
        "prerelease": str(bool(match.group("prerelease")) or release_prerelease).lower(),
    }


if __name__ == "__main__":
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    kind = os.environ["GITHUB_EVENT_NAME"]
    if kind == "release":
        result = metadata(event["release"]["tag_name"], event["release"]["prerelease"])
    elif kind == "workflow_dispatch":
        result = metadata(event["inputs"]["tag"])
    elif kind == "push":
        result = metadata(os.environ["GITHUB_REF_NAME"])
    else:
        raise ValueError(f"Unsupported release event: {kind}")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        for key, value in result.items():
            output.write(f"{key}={value}\n")
