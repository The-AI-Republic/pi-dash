# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stage the bundled CLI for the desktop release (no wildcard executable selection).

Only `pidash` is staged. The built-in engine is owned by the CLI release
(`engine/<version>/` installed by `pidash engine install`) and is never part
of the desktop bundle.
"""

import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

REPO = "The-AI-Republic/pi-dash"
TAG_PREFIX = "pidash-v"


def targets(target):
    if target == "universal-apple-darwin":
        return ["aarch64-apple-darwin", "x86_64-apple-darwin"]
    if target in (
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
    ):
        return [target]
    raise ValueError(f"Unsupported bundle target: {target}")


def normalize_target(target):
    """macOS builds ship universal; normalize single-arch mac triples."""
    if target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
        return "universal-apple-darwin"
    return target


def parse_tag_version(tag):
    """Return (major, minor, patch, prerelease) for a pidash-v* tag."""
    if not tag.startswith(TAG_PREFIX):
        return None
    ver = tag[len(TAG_PREFIX) :]
    # allow leading v as well
    if ver.startswith("v"):
        ver = ver[1:]
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:-(.+))?$", ver)
    if not m:
        return None
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)), m.group(4) or "")


def version_from_tag(tag):
    """Bare version without the pidash-v prefix, e.g. 0.1.30."""
    if tag.startswith(TAG_PREFIX):
        ver = tag[len(TAG_PREFIX) :]
    elif tag.startswith("v"):
        ver = tag[1:]
    else:
        ver = tag
    # validate semver-ish
    if not re.match(r"^\d+\.\d+\.\d+(?:-.+)?$", ver):
        raise ValueError(f"Invalid version tag: {tag}")
    return ver


def resolve_latest_stable_tag(repo=REPO):
    """Newest non-draft, non-prerelease pidash-v* tag, resolved at build time."""
    # Prefer `gh release list` (respects the repo's draft/prerelease flags and
    # lets GitHub order newest-first); fall back to the API when that subcommand
    # is unavailable (older gh).
    candidates = []
    try:
        out = subprocess.check_output(
            [
                "gh",
                "release",
                "list",
                "--repo",
                repo,
                "--exclude-drafts",
                "--exclude-pre-releases",
                "--limit",
                "50",
                "--json",
                "tagName,isDraft,isPrerelease",
            ],
            text=True,
            stderr=subprocess.STDOUT,
        )
        releases = json.loads(out)
        for r in releases:
            name = r.get("tagName") or r.get("tag_name") or ""
            if not name.startswith(TAG_PREFIX):
                continue
            if r.get("isDraft") or r.get("isPrerelease"):
                continue
            pv = parse_tag_version(name)
            if pv is None:
                continue
            candidates.append((pv, name))
    except Exception:
        candidates = []
    if not candidates:
        # Fallback: raw API list
        try:
            out = subprocess.check_output(
                ["gh", "api", f"repos/{repo}/releases?per_page=100"],
                text=True,
                stderr=subprocess.STDOUT,
            )
            releases = json.loads(out)
            for r in releases:
                name = r.get("tag_name") or ""
                if not name.startswith(TAG_PREFIX):
                    continue
                if r.get("draft") or r.get("prerelease"):
                    continue
                pv = parse_tag_version(name)
                if pv is None:
                    continue
                candidates.append((pv, name))
        except Exception:
            pass
    if not candidates:
        raise ValueError(f"No stable {TAG_PREFIX}* release found in {repo}")
    # Newest by semver triple, then pre-release bit (release > prerelease for same triple)
    def key(item):
        (maj, minor, patch, pre), _ = item
        # prerelease sorts *below* the release for the same triple: use
        # (0 if pre else 1) so release wins
        return (maj, minor, patch, 0 if pre else 1)
    candidates.sort(key=key, reverse=True)
    return candidates[0][1]


def write_version_file(output, runner_tag):
    """Record the bundled CLI version for runtime comparison."""
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    version = version_from_tag(runner_tag)
    (output / "pidash.version").write_text(version + "\n", encoding="utf-8")
    return version


def assets(target):
    """Exact release asset and member names, including Windows executable suffixes."""
    windows = target.endswith("windows-msvc")
    suffix = ".exe" if windows else ""
    archive_suffix = ".zip" if windows else ".tar.xz"
    return [
        (
            "The-AI-Republic/pi-dash",
            f"pidash-{target}{archive_suffix}",
            f"pidash{suffix}",
            f"pidash{suffix}",
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


def stage(target, runner_tag, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    target = normalize_target(target)
    with tempfile.TemporaryDirectory(prefix="pidash-agent-slices-") as directory:
        slices = {}
        for architecture in targets(target):
            for repo, archive, member, destination in assets(architecture):
                payload = executable_bytes(
                    download(repo, runner_tag, archive), archive, member
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
    version = write_version_file(output, runner_tag)
    print(f"[bundle-agents] bundled {runner_tag} ({version}) for target {target} -> {output}")
    # In GitHub Actions, surface the resolved tag in the job summary.
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        try:
            with open(summary, "a", encoding="utf-8") as fh:
                fh.write(f"**Bundled pidash CLI**: `{runner_tag}` (`{version}`) for `{target}`\n")
        except OSError:
            pass
    return version


def resolve_runner_tag(pinned, repo=REPO):
    """Explicit pin wins; otherwise resolve latest stable at build time."""
    raw = pinned or os.environ.get("PIDASH_BUNDLE_VERSION") or ""
    raw = raw.strip()
    if raw:
        # Allow bare version or tag; normalize to pidash-v* for the API
        if raw.startswith(TAG_PREFIX):
            tag = raw
        elif raw.startswith("v"):
            tag = TAG_PREFIX + raw[1:]
        elif re.match(r"^\d+\.\d+\.\d+", raw):
            tag = TAG_PREFIX + raw
        else:
            tag = raw
        return tag
    return resolve_latest_stable_tag(repo=repo)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--runner-tag", required=False, default=None)
    parser.add_argument("--repo", default=REPO)
    parser.add_argument("--output", type=Path, default=Path("bin"))
    args = parser.parse_args()
    try:
        runner_tag = resolve_runner_tag(args.runner_tag, repo=args.repo)
    except ValueError as e:
        print(f"[bundle-agents] ERROR: {e}", file=sys.stderr)
        raise SystemExit(1) from e
    print(f"[bundle-agents] resolved runner tag: {runner_tag} (target {args.target})")
    stage(args.target, runner_tag, args.output)


if __name__ == "__main__":
    main()
