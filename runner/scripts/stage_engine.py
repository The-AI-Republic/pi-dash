# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stage the built-in engine for one CLI release target.

The release workflow (`.github/workflows/release.yml`) runs this on each
build-local-artifacts runner before `cargo dist build`. It writes

    runner/dist-engine/engine/<version>/pidash-agent-engine[.exe]
    runner/dist-engine/engine/<version>/LICENSE   (Codex, Apache-2.0)
    runner/dist-engine/engine/<version>/NOTICE    (Codex, verbatim)
    runner/dist-engine/engine/<version>/README.txt (what the file is)

which `include` in `dist-workspace.toml` copies into the target's release
archive as `engine/<version>/`. For Windows targets it also writes
`runner/dist-engine/engine.wxi`, the version define `runner/wix/main.wxs`
includes so the MSI installs the same directory under `bin\\engine\\`.

The engine is the unmodified upstream OpenAI Codex release asset named in
`runner/ENGINE_VERSION`, checked against the SHA-256 digest pinned for it in
`runner/ENGINE_SHA256` (and against the digest GitHub publishes in the
release metadata), then renamed. Only that one regular file is read out
of the upstream archive; nothing the archive names is extracted onto disk.

Needs `gh` with a token (GH_TOKEN), as the desktop scripts do.
"""

import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

RUNNER_DIR = Path(__file__).resolve().parents[1]
VERSION_FILE = RUNNER_DIR / "ENGINE_VERSION"
PINS_FILE = RUNNER_DIR / "ENGINE_SHA256"
DEFAULT_OUT = RUNNER_DIR / "dist-engine"
UPSTREAM = "openai/codex"

# CLI release targets (dist-workspace.toml) this script can stage for.
SUPPORTED_TARGETS = {
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
}


def read_engine_tag(path=VERSION_FILE):
    """The pinned upstream tag, e.g. `rust-v0.153.4`."""
    tag = Path(path).read_text().strip()
    if not tag.startswith("rust-v") or any(c.isspace() for c in tag):
        raise ValueError(f"{path} must hold one upstream tag like rust-v0.153.4, got {tag!r}")
    return tag


def version_from_tag(tag):
    """`rust-v0.153.4` -> `0.153.4`; must match `engine::version_from_tag`."""
    return tag[len("rust-v") :] if tag.startswith("rust-v") else tag


def upstream_asset(target):
    """(asset name, member name, shipped name) for a CLI release target.

    Codex publishes only static musl builds for Linux; they run on glibc
    hosts too, so both Linux variants of the CLI ship the musl engine.
    """
    if target not in SUPPORTED_TARGETS:
        raise ValueError(f"Unsupported release target: {target}")
    windows = target.endswith("windows-msvc")
    suffix = ".exe" if windows else ""
    engine_target = target.replace("linux-gnu", "linux-musl")
    member = f"codex-{engine_target}{suffix}"
    return f"{member}.tar.gz", member, f"pidash-agent-engine{suffix}"


def read_pins(tag, path=PINS_FILE):
    """{asset: "sha256:<hex>"} pinned for `tag` in `runner/ENGINE_SHA256`.

    The file names the tag it was written for; a pin file left behind by an
    ENGINE_VERSION bump is an error, not a silent pass.
    """
    pinned_tag = None
    pins = {}
    for number, raw in enumerate(Path(path).read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        fields = line.split()
        if len(fields) == 2 and fields[0] == "tag":
            pinned_tag = fields[1]
            continue
        digest = fields[0].lower() if fields else ""
        if len(fields) != 2 or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise ValueError(f"{path}:{number}: expected `<sha256>  <asset>`, got {raw!r}")
        pins[fields[1]] = "sha256:" + digest
    if pinned_tag != tag:
        raise ValueError(f"{path} pins digests for {pinned_tag!r}, but ENGINE_VERSION is {tag!r}; update both together")
    return pins


def pinned_digest(pins, asset):
    digest = pins.get(asset)
    if digest is None:
        raise ValueError(f"No SHA-256 pinned for {UPSTREAM}/{asset} in {PINS_FILE.name}")
    return digest


def gh_json(*args):
    return json.loads(subprocess.check_output(["gh", "api", *args], text=True))


def gh_raw(path):
    return subprocess.check_output(
        ["gh", "api", path, "-H", "Accept: application/vnd.github.raw+json"]
    )


def release_digest(metadata, asset):
    digest = next((a.get("digest") for a in metadata["assets"] if a["name"] == asset), None)
    if not digest or not digest.startswith("sha256:"):
        raise ValueError(f"Missing SHA-256 digest for {UPSTREAM}/{asset}")
    return digest


def verify(payload, digest, asset):
    actual = "sha256:" + hashlib.sha256(payload).hexdigest()
    if actual != digest:
        raise ValueError(f"Checksum mismatch for {asset}: expected {digest}, got {actual}")


def executable_bytes(payload, archive, member):
    """Read exactly one regular file named `member` from a .tar.gz in memory."""
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:*") as source:
        matches = [
            item
            for item in source.getmembers()
            if item.isfile() and PurePosixPath(item.name).name == member
        ]
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one {member} in {archive}")
        with source.extractfile(matches[0]) as executable:
            return executable.read()


def download(tag, asset, directory):
    subprocess.run(
        ["gh", "release", "download", tag, "--repo", UPSTREAM, "--pattern", asset, "--dir", str(directory)],
        check=True,
    )
    return (Path(directory) / asset).read_bytes()


README = """pidash-agent-engine is OpenAI Codex {tag} (https://github.com/openai/codex),
unmodified and renamed, licensed under the Apache License 2.0 (see LICENSE and
NOTICE in this directory). Pi Dash runs it as its built-in agent engine. It is
installed outside PATH on purpose; the pidash daemon finds it by path.
"""

WXI = """<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by runner/scripts/stage_engine.py from runner/ENGINE_VERSION. -->
<Include>
    <?define EngineVersion = "{version}" ?>
</Include>
"""


def stage(target, out, tag, pins, fetch=None):
    """Write the engine directory for `target` under `out`; return its path.

    `pins` is `read_pins(tag)`. `fetch(tag, asset) -> (payload, digest,
    license, notice)` is injectable for tests; the default talks to GitHub
    through `gh`. The payload must match both the pinned digest and the one
    the upstream release metadata reports.
    """
    asset, member, shipped = upstream_asset(target)
    version = version_from_tag(tag)
    expected = pinned_digest(pins, asset)
    payload, digest, license_text, notice_text = (fetch or fetch_from_github)(tag, asset)
    if digest != expected:
        raise ValueError(
            f"Upstream metadata for {asset} reports {digest}, but {PINS_FILE.name} pins {expected}"
        )
    verify(payload, expected, asset)
    binary = executable_bytes(payload, asset, member)

    out = Path(out)
    engine_root = out / "engine"
    if engine_root.exists():
        # One target per staging run: a stale version or another target's
        # binary must never ride into this archive.
        shutil.rmtree(engine_root)
    directory = engine_root / version
    directory.mkdir(parents=True)
    path = directory / shipped
    path.write_bytes(binary)
    path.chmod(0o755)
    (directory / "LICENSE").write_bytes(license_text)
    (directory / "NOTICE").write_bytes(notice_text)
    (directory / "README.txt").write_text(README.format(tag=tag))

    wxi = out / "engine.wxi"
    if target.endswith("windows-msvc"):
        wxi.write_text(WXI.format(version=version))
    elif wxi.exists():
        wxi.unlink()
    return directory


def fetch_from_github(tag, asset):
    metadata = gh_json(f"repos/{UPSTREAM}/releases/tags/{tag}")
    digest = release_digest(metadata, asset)
    with tempfile.TemporaryDirectory(prefix="pidash-engine-") as directory:
        payload = download(tag, asset, directory)
    license_text = gh_raw(f"repos/{UPSTREAM}/contents/LICENSE?ref={tag}")
    notice_text = gh_raw(f"repos/{UPSTREAM}/contents/NOTICE?ref={tag}")
    return payload, digest, license_text, notice_text


def smoke(directory, target):
    """Run `--version` when this host can execute the target's binary."""
    _, _, shipped = upstream_asset(target)
    binary = Path(directory) / shipped
    host_os = {"darwin": "apple-darwin", "win32": "windows"}.get(sys.platform, "linux")
    machine = platform.machine().lower()
    host_arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    if host_os not in target or not target.startswith(host_arch):
        return None
    result = subprocess.run([str(binary), "--version"], capture_output=True, text=True, check=True)
    return result.stdout.strip()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", required=True, help="CLI release target triple")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--version-file", type=Path, default=VERSION_FILE)
    parser.add_argument("--pins-file", type=Path, default=PINS_FILE)
    parser.add_argument("--no-smoke", action="store_true", help="skip running --version")
    args = parser.parse_args(argv)

    tag = read_engine_tag(args.version_file)
    pins = read_pins(tag, args.pins_file)
    directory = stage(args.target, args.out, tag, pins)
    print(f"[stage-engine] {args.target}: staged {tag} at {directory}")
    if not args.no_smoke:
        version = smoke(directory, args.target)
        if version is not None:
            print(f"[stage-engine] {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
