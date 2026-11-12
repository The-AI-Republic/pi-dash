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
    runner/dist-engine/engine/<version>/bwrap      (Linux only)
    runner/dist-engine/engine/<version>/codex-command-runner.exe        (Windows only)
    runner/dist-engine/engine/<version>/codex-windows-sandbox-setup.exe (Windows only)

which `include` in `dist-workspace.toml` copies into the target's release
archive as `engine/<version>/`. For Windows targets it also writes
`runner/dist-engine/engine.wxi`, the version define `runner/wix/main.wxs`
includes so the MSI installs the same directory under `bin\\engine\\`.

The engine is the unmodified upstream OpenAI Codex release asset named in
`runner/ENGINE_VERSION`, checked against the SHA-256 digest pinned for it in
`runner/ENGINE_SHA256` (and against the digest GitHub publishes in the
release metadata), then renamed. The sandbox helpers next to it —
`bwrap` on Linux, `codex-command-runner.exe` and
`codex-windows-sandbox-setup.exe` on Windows — are the engine's own
unmodified release assets, fetched and verified the same way: the engine
resolves them beside its own executable when no system helper is
available (PIDESKAPP-148, PIDESKAPP-109), and a system bwrap alone does
not fix Windows. macOS needs none (system seatbelt). Only those regular
files are read out of the upstream archives; nothing the archives name
is extracted onto disk.

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


def upstream_helpers(target):
    """[(asset name, member name, shipped name)] for the sandbox helpers.

    The engine finds these beside its own executable (`bwrap` and the
    Windows helpers are looked up in the executable's directory, so the
    rename to `pidash-agent-engine` cannot break resolution). Linux
    publishes only static musl helper builds, matching the engine; the
    Windows helper assets are per-architecture like the engine, and only
    the staged target's are fetched. macOS sandboxes through the system
    seatbelt and needs no helper.
    """
    if target not in SUPPORTED_TARGETS:
        raise ValueError(f"Unsupported release target: {target}")
    if target.endswith("windows-msvc"):
        return [
            (
                f"codex-command-runner-{target}.exe.tar.gz",
                f"codex-command-runner-{target}.exe",
                "codex-command-runner.exe",
            ),
            (
                f"codex-windows-sandbox-setup-{target}.exe.tar.gz",
                f"codex-windows-sandbox-setup-{target}.exe",
                "codex-windows-sandbox-setup.exe",
            ),
        ]
    if "unknown-linux" in target:
        engine_target = target.replace("linux-gnu", "linux-musl")
        return [(f"bwrap-{engine_target}.tar.gz", f"bwrap-{engine_target}", "bwrap")]
    return []


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

{note}
"""


def readme_text(target, tag):
    helpers = [shipped for _, _, shipped in upstream_helpers(target)]
    if helpers:
        note = (
            "The helper program(s) in this directory are the same Codex release's "
            f"sandbox helpers ({', '.join(helpers)}): OpenAI Codex finds them beside its own "
            "executable, so they sail along with the renamed engine when no system "
            "helper is installed."
        )
    else:
        note = "macOS sandboxes through the system seatbelt, so this directory holds no extra helper program."
    return README.format(tag=tag, note=note)

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

    # The engine's sandbox helpers ship beside the engine: the same
    # metadata-vs-pin verification as the engine itself. An injected
    # `fetch` (tests) serves every asset; the production path fetches
    # helper assets without re-fetching LICENSE/NOTICE for each.
    helpers = []
    for helper_asset, helper_member, helper_shipped in upstream_helpers(target):
        helper_expected = pinned_digest(pins, helper_asset)
        if fetch is None:
            helper_payload, helper_digest = fetch_asset(tag, helper_asset)
        else:
            helper_payload, helper_digest, _, _ = fetch(tag, helper_asset)
        if helper_digest != helper_expected:
            raise ValueError(
                f"Upstream metadata for {helper_asset} reports {helper_digest}, but {PINS_FILE.name} pins {helper_expected}"
            )
        verify(helper_payload, helper_expected, helper_asset)
        helpers.append((helper_shipped, executable_bytes(helper_payload, helper_asset, helper_member)))

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
    for helper_shipped, helper_bytes in helpers:
        helper_path = directory / helper_shipped
        helper_path.write_bytes(helper_bytes)
        helper_path.chmod(0o755)
    (directory / "LICENSE").write_bytes(license_text)
    (directory / "NOTICE").write_bytes(notice_text)
    (directory / "README.txt").write_text(readme_text(target, tag))

    wxi = out / "engine.wxi"
    if target.endswith("windows-msvc"):
        wxi.write_text(WXI.format(version=version))
    elif wxi.exists():
        wxi.unlink()
    return directory


def fetch_asset(tag, asset):
    """(payload, release-metadata digest) for one upstream asset."""
    metadata = gh_json(f"repos/{UPSTREAM}/releases/tags/{tag}")
    digest = release_digest(metadata, asset)
    with tempfile.TemporaryDirectory(prefix="pidash-engine-") as directory:
        payload = download(tag, asset, directory)
    return payload, digest


def fetch_licenses(tag):
    """(license, notice) for the tag, verbatim from the upstream repo."""
    license_text = gh_raw(f"repos/{UPSTREAM}/contents/LICENSE?ref={tag}")
    notice_text = gh_raw(f"repos/{UPSTREAM}/contents/NOTICE?ref={tag}")
    return license_text, notice_text


def fetch_from_github(tag, asset):
    payload, digest = fetch_asset(tag, asset)
    license_text, notice_text = fetch_licenses(tag)
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
