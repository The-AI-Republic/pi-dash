# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stage pinned agent release assets for the desktop (no wildcard executable selection)."""

import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tempfile
import zipfile

# The pinned agent-engine release, shared with prepare-agent.sh, the CLI
# release pipeline (runner/scripts/stage_engine.py) and the runner itself.
ENGINE_VERSION_FILE = Path(__file__).resolve().parents[2] / "runner/ENGINE_VERSION"
# SHA-256 of each upstream Codex asset, pinned in the repo next to
# ENGINE_VERSION. The digest GitHub publishes must match it, so a replaced
# upstream asset fails the bundle the same way it fails the CLI release.
ENGINE_SHA256_FILE = ENGINE_VERSION_FILE.with_name("ENGINE_SHA256")
STAGE_ENGINE = ENGINE_VERSION_FILE.parent / "scripts/stage_engine.py"


def _stage_engine():
    spec = importlib.util.spec_from_file_location("stage_engine", STAGE_ENGINE)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def engine_pins(tag, path=ENGINE_SHA256_FILE):
    """{asset: "sha256:<hex>"} pinned for `tag` (same parser as the CLI release)."""
    return _stage_engine().read_pins(tag, path)


def default_engine_tag(path=ENGINE_VERSION_FILE):
    return Path(path).read_text().strip()


def targets(target):
    if target == "universal-apple-darwin":
        return ["aarch64-apple-darwin", "x86_64-apple-darwin"]
    if target in ("x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"):
        return [target]
    raise ValueError(f"Unsupported bundle target: {target}")


def assets(target):
    """Exact upstream asset and member names, including Windows executable suffixes."""
    windows = target.endswith("windows-msvc")
    suffix = ".exe" if windows else ""
    engine_target = target.replace("linux-gnu", "linux-musl")
    archive_suffix = ".zip" if windows else ".tar.xz"
    return [
        (
            "The-AI-Republic/pi-dash",
            f"pidash-{target}{archive_suffix}",
            f"pidash{suffix}",
            f"pidash{suffix}",
        ),
        (
            "openai/codex",
            f"codex-{engine_target}{suffix}.tar.gz",
            f"codex-{engine_target}{suffix}",
            f"pidash-agent-engine{suffix}",
        ),
    ]


def download(repo, tag, asset, pinned=None):
    """Download one release asset, checked against GitHub's digest and, when
    given, the digest pinned in the repo."""
    metadata = json.loads(
        subprocess.check_output(["gh", "api", f"repos/{repo}/releases/tags/{tag}"], text=True)
    )
    digest = next((a.get("digest") for a in metadata["assets"] if a["name"] == asset), None)
    if not digest or not digest.startswith("sha256:"):
        raise ValueError(f"Missing SHA-256 digest for {repo}@{tag}/{asset}")
    if pinned is not None and digest != pinned:
        raise ValueError(
            f"{repo}@{tag}/{asset} publishes {digest}, but the repo pins {pinned}"
        )
    with tempfile.TemporaryDirectory(prefix="pidash-agent-asset-") as directory:
        subprocess.run(
            [
                "gh",
                "release",
                "download",
                tag,
                "--repo",
                repo,
                "--pattern",
                asset,
                "--dir",
                directory,
            ],
            check=True,
        )
        payload = (Path(directory) / asset).read_bytes()
    if f"sha256:{hashlib.sha256(payload).hexdigest()}" != digest:
        raise ValueError(f"Checksum mismatch for {asset}")
    return payload


def executable_bytes(payload, archive, member):
    # Read exactly one regular executable into memory. Never extract paths or
    # symlinks supplied by the archive onto the build host.
    if archive.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(payload)) as source:
            matches = [
                item
                for item in source.infolist()
                if not item.is_dir() and PurePosixPath(item.filename).name == member
            ]
            if len(matches) != 1:
                raise ValueError(f"Expected exactly one {member} in {archive}")
            return source.read(matches[0])
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


def stage(target, runner_tag, engine_tag, output, pins_file=ENGINE_SHA256_FILE):
    pins = engine_pins(engine_tag, pins_file)
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pidash-agent-slices-") as directory:
        slices = {}
        for architecture in targets(target):
            for repo, archive, member, destination in assets(architecture):
                if repo == "openai/codex":
                    tag = engine_tag
                    pinned = pins.get(archive)
                    if pinned is None:
                        raise ValueError(f"No SHA-256 pinned for {repo}/{archive} in {pins_file}")
                else:
                    tag, pinned = runner_tag, None
                payload = executable_bytes(
                    download(repo, tag, archive, pinned=pinned), archive, member
                )
                path = Path(directory) / architecture / destination
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(payload)
                path.chmod(0o755)
                slices.setdefault(destination, []).append(path)
        for name, paths in slices.items():
            destination = output / name
            if len(paths) == 2:
                subprocess.run(
                    ["lipo", "-create", *map(str, paths), "-output", str(destination)], check=True
                )
                subprocess.run(
                    ["lipo", str(destination), "-verify_arch", "arm64", "x86_64"], check=True
                )
            else:
                destination.write_bytes(paths[0].read_bytes())
            destination.chmod(0o755)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--runner-tag", required=True)
    parser.add_argument(
        "--engine-tag",
        help="upstream openai/codex tag; defaults to runner/ENGINE_VERSION",
    )
    parser.add_argument("--output", type=Path, default=Path("bin"))
    args = parser.parse_args()
    engine_tag = args.engine_tag or default_engine_tag()
    stage(args.target, args.runner_tag, engine_tag, args.output)


if __name__ == "__main__":
    main()
