#!/usr/bin/env python3
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Publication gate for public refs and artifacts.

Nothing reaches the public repository or a public registry until it has been
built from a commit already on main, transformed into its final form, scanned
and tested, and then handed, byte for byte, to the protected publisher job.
See README.md next to this file.

Subcommands (all exit non-zero on failure):

  check-policy                      workflows and the tag ruleset obey the gate
  scan [--denylist-env VAR] PATH..  reject secrets and denylisted markers
  self-test [--denylist-env VAR]    prove the scanner rejects a planted leak
  build-manifest DIR --tag --sha    write DIR's file list and digests
  verify-manifest DIR [--expect-digest D]
                                    DIR holds exactly the manifest's files
  check-live-ruleset --repo --app-id
                                    the live tag ruleset matches the policy

Only the standard library is needed, except check-policy (PyYAML).
"""

from __future__ import annotations

import argparse
import base64
import bz2
import dataclasses
import fnmatch
import gzip
import hashlib
import io
import json
import lzma
import os
import pathlib
import re
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
RULESET_PATH = ".github/publication-gate/ruleset.json"
ALLOWLIST_PATH = ".github/publication-gate/allowlist.txt"
MANIFEST_NAME = "publication-manifest.json"

# Workflows that build public artifacts. Every other workflow must not publish.
PUBLISHING_WORKFLOWS = ("release.yml", "build-branch.yml")

PUBLISHER_ENVIRONMENT = "release-publisher"
SCAN_ENVIRONMENT = "publication-scan"
# Secrets a job may read, by the environment it runs in. GITHUB_TOKEN is
# always allowed: every non-publisher job holds it read-only.
SECRETS_BY_ENVIRONMENT = {
    None: {"GITHUB_TOKEN"},
    SCAN_ENVIRONMENT: {"GITHUB_TOKEN", "PUBLICATION_DENYLIST"},
}

# Shell fragments and actions that create refs, releases or public images.
PUBLISHING_COMMANDS = [
    (re.compile(r"\bgit\s+push\b"), "git push"),
    (re.compile(r"\bgit\s+tag\b(?!\s+(?:--list|-l)\b)"), "git tag"),
    (re.compile(r"\bgh\s+release\s+(create|upload|edit)\b"), "gh release"),
    (re.compile(r"/git/(refs|tags)\b"), "git refs API"),
    (re.compile(r"\bskopeo\s+copy\b"), "skopeo copy"),
    (re.compile(r"\bdocker\s+(image\s+)?push\b"), "docker push"),
    (re.compile(r"\bcrane\s+(push|copy)\b"), "crane push"),
    (re.compile(r"\bput-parameter\b"), "ssm put-parameter"),
]
PUBLISHING_ACTIONS = ("softprops/action-gh-release", "docker/login-action", "aws-actions/configure-aws-credentials")

# Tag ruleset: every release tag shape, and the rules that stop anyone but
# the publisher from creating, moving or deleting one.
REQUIRED_TAG_PATTERNS = ("refs/tags/pidash-v*", "refs/tags/platform-v*", "refs/tags/v*")
REQUIRED_RULES = {"creation", "update", "deletion", "non_fast_forward"}


# --------------------------------------------------------------------------
# Workflow policy
# --------------------------------------------------------------------------


def load_yaml(path):
    import yaml

    return yaml.safe_load(pathlib.Path(path).read_text())


def _triggers(doc):
    # PyYAML reads a bare `on:` key as the boolean True.
    on = doc.get("on", doc.get(True)) if isinstance(doc, dict) else None
    if isinstance(on, str):
        return {on: None}
    if isinstance(on, list):
        return {e: None for e in on}
    return on or {}


def trigger_problems(name, doc):
    """A workflow must not start on a tag push, ref creation or release."""
    problems = []
    on = _triggers(doc)
    for event in ("create", "release", "registry_package"):
        if event in on:
            problems.append(f"{name}: starts on '{event}' events, so creating a ref or release could publish")
    push = on.get("push")
    # A push trigger that does not filter on branches also fires on tag
    # pushes; path filters are not evaluated for tags.
    if "push" in on and (
        not isinstance(push, dict)
        or "tags" in push
        or "tags-ignore" in push
        or not ("branches" in push or "branches-ignore" in push)
    ):
        problems.append(f"{name}: starts on tag pushes, so creating a tag could publish")
    return problems


def _environment(job):
    env = job.get("environment")
    if isinstance(env, dict):
        env = env.get("name")
    return env


def _strings(node):
    if isinstance(node, str):
        yield node
    elif isinstance(node, dict):
        for k, v in node.items():
            yield from _strings(k)
            yield from _strings(v)
    elif isinstance(node, list):
        for v in node:
            yield from _strings(v)


def _expand_steps(steps, repo_root, seen=()):
    """Steps of a job, with local composite actions inlined."""
    out = []
    for step in steps or []:
        out.append(step)
        uses = step.get("uses", "") if isinstance(step, dict) else ""
        if uses.startswith("./") and uses not in seen:
            action_dir = pathlib.Path(repo_root) / uses[2:]
            for fname in ("action.yml", "action.yaml"):
                if (action_dir / fname).exists():
                    action = load_yaml(action_dir / fname) or {}
                    out += _expand_steps((action.get("runs") or {}).get("steps"), repo_root, seen + (uses,))
                    break
    return out


def _writes(perms):
    if perms in ("write-all",):
        return ["write-all"]
    if isinstance(perms, dict):
        return [k for k, v in perms.items() if v == "write"]
    return []


def policy_problems(name, doc, repo_root=REPO_ROOT):
    """Check one publishing workflow against the gate."""
    problems = list(trigger_problems(name, doc))
    top_perms = doc.get("permissions")
    if top_perms is None:
        problems.append(f"{name}: no top-level permissions; the token default may be write")
    elif _writes(top_perms):
        problems.append(f"{name}: top-level permissions grant write ({', '.join(_writes(top_perms))})")
    for text in _strings(doc.get("env") or {}):
        if "secrets." in text:
            problems.append(f"{name}: workflow-level env reads a secret")

    jobs = doc.get("jobs") or {}
    publishers = [j for j, job in jobs.items() if _environment(job) == PUBLISHER_ENVIRONMENT]
    scanners = [j for j, job in jobs.items() if any(" scan" in s and "gate.py" in s for s in _strings(job.get("steps")))]

    for job_name, job in jobs.items():
        env = _environment(job)
        is_publisher = env == PUBLISHER_ENVIRONMENT
        where = f"{name}: job '{job_name}'"
        if "uses" in job and not is_publisher and job.get("secrets"):
            problems.append(f"{where}: passes secrets to a reusable workflow")
        steps = _expand_steps(job.get("steps"), repo_root)
        texts = list(_strings({k: v for k, v in job.items() if k != "steps"})) + list(_strings(steps))
        if not is_publisher:
            allowed = SECRETS_BY_ENVIRONMENT.get(env, SECRETS_BY_ENVIRONMENT[None])
            for text in texts:
                for secret in re.findall(r"secrets\.([A-Za-z_][A-Za-z0-9_]*)", text):
                    if secret not in allowed:
                        problems.append(f"{where}: reads secret {secret} outside the protected publisher")
                if re.search(r"secrets\s*\[", text):
                    problems.append(f"{where}: reads a secret by index outside the protected publisher")
            if _writes(job.get("permissions")):
                problems.append(f"{where}: write permissions outside the protected publisher")
            for step in steps:
                if not isinstance(step, dict):
                    continue
                uses = step.get("uses", "")
                if any(uses.startswith(a) for a in PUBLISHING_ACTIONS):
                    problems.append(f"{where}: uses {uses} outside the protected publisher")
                with_ = step.get("with") or {}
                if str(with_.get("push", "")).lower() == "true":
                    problems.append(f"{where}: pushes an image outside the protected publisher")
                run = step.get("run", "") or ""
                for pattern, label in PUBLISHING_COMMANDS:
                    if pattern.search(run):
                        problems.append(f"{where}: runs '{label}' outside the protected publisher")
        else:
            needs = job.get("needs") or []
            needs = [needs] if isinstance(needs, str) else needs
            if not set(needs) & set(scanners):
                problems.append(f"{where}: the publisher does not need the scan job")
            runs = [s.get("run", "") or "" for s in steps if isinstance(s, dict)]
            verify_at = next((i for i, r in enumerate(runs) if "gate.py verify-manifest" in r), None)
            if verify_at is None:
                problems.append(f"{where}: the publisher never runs gate.py verify-manifest")
            else:
                for r in runs[:verify_at]:
                    for pattern, label in PUBLISHING_COMMANDS:
                        if pattern.search(r):
                            problems.append(f"{where}: runs '{label}' before verify-manifest")
    if not publishers and any(
        pattern.search(t) for job in jobs.values() for t in _strings(job) for pattern, _ in PUBLISHING_COMMANDS
    ):
        problems.append(f"{name}: publishes without a '{PUBLISHER_ENVIRONMENT}' job")
    return problems


def workflow_problems(path, repo_root=REPO_ROOT):
    path = pathlib.Path(path)
    return policy_problems(path.name, load_yaml(path), repo_root)


# --------------------------------------------------------------------------
# Tag ruleset
# --------------------------------------------------------------------------


def _ref_glob_match(pattern, ref):
    # GitHub ref patterns use fnmatch-style globs where * stops at '/'.
    regex = re.escape(pattern).replace(r"\*\*", ".*").replace(r"\*", "[^/]*")
    return re.fullmatch(regex, ref) is not None


def ruleset_covers(ruleset, ref):
    cond = (ruleset.get("conditions") or {}).get("ref_name") or {}
    included = any(_ref_glob_match(p, ref) for p in cond.get("include") or [])
    excluded = any(_ref_glob_match(p, ref) for p in cond.get("exclude") or [])
    return included and not excluded


def ruleset_problems(ruleset, app_id=None):
    problems = []
    if ruleset.get("target") != "tag":
        problems.append("ruleset target must be 'tag'")
    if ruleset.get("enforcement") != "active":
        problems.append("ruleset enforcement must be 'active'")
    cond = (ruleset.get("conditions") or {}).get("ref_name") or {}
    for pattern in REQUIRED_TAG_PATTERNS:
        if pattern not in (cond.get("include") or []):
            problems.append(f"ruleset does not include {pattern}")
    if cond.get("exclude"):
        problems.append("ruleset must not exclude any tag")
    types = {r.get("type") for r in ruleset.get("rules") or []}
    for rule in sorted(REQUIRED_RULES - types):
        problems.append(f"ruleset lacks the '{rule}' rule")
    actors = ruleset.get("bypass_actors") or []
    if len(actors) != 1:
        problems.append("ruleset must have exactly one bypass actor: the publisher app")
    for actor in actors:
        if actor.get("actor_type") != "Integration":
            problems.append(f"bypass actor {actor.get('actor_type')} is not the publisher app")
        if app_id is not None and str(actor.get("actor_id")) != str(app_id):
            problems.append("bypass actor is not the configured publisher app")
    return problems


def live_ruleset_problems(rulesets, app_id):
    """Problems with the rulesets GitHub reports as live for this repo."""
    gate_sets = [r for r in rulesets if r.get("target") == "tag" and not ruleset_problems(r, app_id)]
    if gate_sets:
        return []
    if not rulesets:
        return ["no live tag ruleset found"]
    detail = []
    for r in rulesets:
        if r.get("target") == "tag":
            detail += [f"{r.get('name')}: {p}" for p in ruleset_problems(r, app_id)]
    return detail or ["no live tag ruleset found"]


def _gh_get(url, token):
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.load(resp)


def fetch_live_rulesets(repo, token):
    api = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    listing = _gh_get(f"{api}/repos/{repo}/rulesets?includes_parents=true&per_page=100", token)
    return [_gh_get(f"{api}/repos/{repo}/rulesets/{r['id']}", token) for r in listing]


# --------------------------------------------------------------------------
# Content scanner
# --------------------------------------------------------------------------

# Generic credential shapes. Kept deliberately specific so that compiled
# binaries do not trip them by chance.
SECRET_RULES = [
    # A key header followed by key material, so PEM parsers that merely
    # name the header (common in compiled binaries) do not match.
    ("private-key", re.compile(rb"-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED |PGP )?PRIVATE KEY(?: BLOCK)?-----\s*[A-Za-z0-9+/]{32}")),
    ("aws-access-key", re.compile(rb"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("github-token", re.compile(rb"\bgh[pousr]_[A-Za-z0-9]{36,}\b")),
    ("github-pat", re.compile(rb"\bgithub_pat_[A-Za-z0-9_]{60,}\b")),
    ("slack-token", re.compile(rb"\bxox[abprs]-[A-Za-z0-9-]{10,}")),
    ("anthropic-key", re.compile(rb"\bsk-ant-[A-Za-z0-9_-]{20,}")),
    ("google-api-key", re.compile(rb"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("dockerhub-pat", re.compile(rb"\bdckr_pat_[A-Za-z0-9_-]{20,}")),
    ("stripe-live-key", re.compile(rb"\b[sr]k_live_[0-9A-Za-z]{24,}\b")),
]
# Well-known placeholder values from public documentation.
PUBLIC_EXAMPLES = {b"AKIAIOSFODNN7EXAMPLE"}
# Vendored third-party trees carry their own test keys. Generic secret rules
# skip them; the denylist still applies everywhere.
THIRD_PARTY_PARTS = ("node_modules", "site-packages", "dist-packages", ".cargo", "vendor")
THIRD_PARTY_PREFIXES = ("usr/lib/", "usr/share/", "usr/local/lib/", "lib/", "etc/ssl/")

MAX_DEPTH = 8
# Zip members need random access, so a zip is read into memory; cap it.
MAX_ZIP_BYTES = 1024 * 1024 * 1024
CHUNK = 8 * 1024 * 1024
OVERLAP = 4096


@dataclasses.dataclass
class Finding:
    rule: str
    location: str
    fingerprint: str

    def __str__(self):
        # Never print the matched value: the report itself is public CI output.
        return f"{self.rule}: {self.location} (fingerprint {self.fingerprint})"


def parse_allowlist(text):
    """Lines of `<rule> <path-glob>`; '#' starts a comment."""
    entries = []
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            rule, _, glob = line.partition(" ")
            entries.append((rule.strip(), glob.strip()))
    return entries


def load_allowlist(root=REPO_ROOT):
    path = pathlib.Path(root) / ALLOWLIST_PATH
    return parse_allowlist(path.read_text()) if path.exists() else []


def denylist_from_env(var):
    """The denylist is a base64 secret, one literal marker per line.

    It never lives in this repository, so the markers it guards stay private.
    """
    raw = os.environ.get(var, "")
    if not raw.strip():
        return []
    text = base64.b64decode(raw).decode("utf-8")
    return [line.strip() for line in text.splitlines() if line.strip() and not line.strip().startswith("#")]


def _is_third_party(path):
    parts = path.replace("\\", "/").split("/")
    inner = path.replace("\\", "/").rsplit("!/", 1)[-1].lstrip("/")
    return any(p in THIRD_PARTY_PARTS for p in parts) or inner.startswith(THIRD_PARTY_PREFIXES)


def _allowed(rule, location, allowlist):
    inner = location.replace("\\", "/")
    for a_rule, glob in allowlist:
        if a_rule in (rule, "*") and (fnmatch.fnmatch(inner, glob) or fnmatch.fnmatch(inner, f"*/{glob}")):
            return True
    return False


def _fingerprint(rule, value):
    return hashlib.sha256(rule.encode() + b"\0" + value).hexdigest()[:12]


class _Chain(io.RawIOBase):
    """A read-only stream: some already-read bytes, then the rest of a stream."""

    def __init__(self, head, rest):
        self._head = head
        self._rest = rest

    def readable(self):
        return True

    def readinto(self, buf):
        if self._head:
            n = min(len(buf), len(self._head))
            buf[:n] = self._head[:n]
            self._head = self._head[n:]
            return n
        data = self._rest.read(len(buf))
        buf[: len(data)] = data
        return len(data)


def _read_full(fh, n):
    out = b""
    while len(out) < n:
        piece = fh.read(n - len(out))
        if not piece:
            break
        out += piece
    return out


def _scan_stream(fh, location, ctx, depth=0):
    """Scan a stream, recursing into compressed data and archives.

    Archives are read member by member, so multi-gigabyte image archives and
    their layers are never held in memory whole.
    """
    if depth > MAX_DEPTH:
        ctx["findings"].append(Finding("too-deep", location, "-"))
        return
    head = _read_full(fh, 512)
    stream = io.BufferedReader(_Chain(head, fh), buffer_size=1 << 20)

    if head[:2] == b"\x1f\x8b" or head[:6] == b"\xfd7zXZ\x00" or head[:3] == b"BZh":
        if head[:2] == b"\x1f\x8b":
            inner = gzip.GzipFile(fileobj=stream)
        elif head[:3] == b"BZh":
            inner = bz2.BZ2File(stream)
        else:
            inner = lzma.LZMAFile(stream)
        try:
            _scan_stream(inner, location, ctx, depth + 1)
            return
        except (OSError, EOFError, lzma.LZMAError, ValueError):
            # Not really compressed data (or truncated): scan the raw bytes.
            ctx["findings"].append(Finding("unreadable", location, "-"))
            return

    if head[:4] == b"PK\x03\x04":
        data = head + _read_full(stream, MAX_ZIP_BYTES)
        if len(data) > MAX_ZIP_BYTES:
            ctx["findings"].append(Finding("too-large", location, "-"))
            return
        try:
            with zipfile.ZipFile(io.BytesIO(data)) as zf:
                for info in zf.infolist():
                    if not info.is_dir():
                        with zf.open(info) as member:
                            _scan_stream(member, f"{location}!/{info.filename}", ctx, depth + 1)
            return
        except zipfile.BadZipFile:
            stream = io.BufferedReader(_Chain(data, io.BytesIO(b"")))

    elif len(head) > 262 and head[257:262] == b"ustar":
        try:
            with tarfile.open(fileobj=stream, mode="r|") as tar:
                for member in tar:
                    if member.isfile():
                        _scan_stream(tar.extractfile(member), f"{location}!/{member.name}", ctx, depth + 1)
            return
        except tarfile.TarError:
            ctx["findings"].append(Finding("unreadable", location, "-"))
            return

    _scan_content(stream, location, ctx)


def _scan_content(stream, location, ctx):
    denylist, allowlist, findings = ctx["denylist"], ctx["allowlist"], ctx["findings"]
    third_party = _is_third_party(location)
    hit = set()
    needles = [m.encode("utf-8").lower() for m in denylist if m]
    tail = b""
    while True:
        chunk = stream.read(CHUNK)
        if not chunk:
            break
        window = tail + chunk
        if not third_party:
            for rule, pattern in SECRET_RULES:
                if rule in hit:
                    continue
                for match in pattern.finditer(window):
                    value = match.group(0)
                    if value in PUBLIC_EXAMPLES or _allowed(rule, location, allowlist):
                        continue
                    findings.append(Finding(rule, location, _fingerprint(rule, value)))
                    hit.add(rule)
                    break
        if needles:
            lowered = window.lower()
            for needle in needles:
                key = b"denylist:" + needle
                if key not in hit and needle in lowered:
                    findings.append(Finding("denylist", location, _fingerprint("denylist", needle)))
                    hit.add(key)
        tail = window[-OVERLAP:]


def _scan_bytes(data, location, denylist, allowlist, findings, depth=0):
    ctx = {"denylist": denylist, "allowlist": allowlist, "findings": findings}
    _scan_stream(io.BytesIO(data), location, ctx, depth)


def scan_paths(paths, denylist, allowlist):
    """Scan files and directories, opening archives and compressed blobs."""
    findings = []
    ctx = {"denylist": denylist, "allowlist": allowlist, "findings": findings}
    for root in paths:
        root = pathlib.Path(root)
        if root.is_file():
            files = [(root, root.name)]
        else:
            files = [
                (p, p.relative_to(root).as_posix())
                for p in sorted(root.rglob("*"))
                if p.is_file() and not p.is_symlink() and ".git" not in p.relative_to(root).parts
            ]
        for path, rel in files:
            with open(path, "rb") as fh:
                _scan_stream(fh, rel, ctx)
    return findings


def self_test(denylist):
    """Plant a synthetic leak and prove the scanner rejects it.

    Runs before every real scan, so a scanner that silently stopped matching
    (a broken rule, an empty denylist) fails the release instead of passing it.
    """
    # Joined at runtime so neither this file nor its bytecode holds a match.
    leaks = ["".join(["AKIA", "Q7ZW" * 4]), "".join(["gh", "p_", "Zz9" * 12])]
    leaks += denylist
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        for i, leak in enumerate(leaks):
            payload = f"value={leak}\n".encode()
            buf = io.BytesIO()
            with tarfile.open(fileobj=buf, mode="w:gz") as tar:
                info = tarfile.TarInfo(f"etc/app/planted-{i}.env")
                info.size = len(payload)
                tar.addfile(info, io.BytesIO(payload))
            (tmp / f"planted-{i}.tar.gz").write_bytes(buf.getvalue())
            if not scan_paths([tmp / f"planted-{i}.tar.gz"], denylist=[leak] if leak in denylist else [], allowlist=[]):
                return False
        clean = tmp / "clean.txt"
        clean.write_text("nothing to see\n")
        return scan_paths([clean], denylist=denylist, allowlist=[]) == []


# --------------------------------------------------------------------------
# Manifest: the exact set of files that was scanned
# --------------------------------------------------------------------------


def _sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _candidate_files(directory):
    directory = pathlib.Path(directory)
    return sorted(p.relative_to(directory).as_posix() for p in directory.rglob("*") if p.is_file() and p.name != MANIFEST_NAME)


def build_manifest(directory, tag, sha, extra=None):
    directory = pathlib.Path(directory)
    manifest = {
        "tag": tag,
        "sha": sha,
        "files": {rel: _sha256_file(directory / rel) for rel in _candidate_files(directory)},
    }
    if extra:
        manifest.update(extra)
    return manifest


def manifest_digest(manifest):
    return hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def verify_manifest(directory, manifest):
    directory = pathlib.Path(directory)
    problems = []
    expected = manifest.get("files") or {}
    present = set(_candidate_files(directory))
    for rel in sorted(present - set(expected)):
        problems.append(f"unscanned file: {rel}")
    for rel, digest in sorted(expected.items()):
        if rel not in present:
            problems.append(f"missing file: {rel}")
        elif _sha256_file(directory / rel) != digest:
            problems.append(f"changed since scan: {rel}")
    return problems


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def check_policy(repo_root=REPO_ROOT):
    repo_root = pathlib.Path(repo_root)
    problems = []
    for path in sorted((repo_root / ".github" / "workflows").glob("*.y*ml")):
        doc = load_yaml(path)
        if path.name in PUBLISHING_WORKFLOWS:
            problems += policy_problems(path.name, doc, repo_root)
        else:
            problems += trigger_problems(path.name, doc)
            for job_name, job in (doc.get("jobs") or {}).items():
                for step in _expand_steps(job.get("steps"), repo_root):
                    run = (step.get("run", "") if isinstance(step, dict) else "") or ""
                    uses = (step.get("uses", "") if isinstance(step, dict) else "") or ""
                    if any(uses.startswith(a) for a in PUBLISHING_ACTIONS[:1]) or re.search(r"\bgh\s+release\s+create\b", run):
                        problems.append(f"{path.name}: job '{job_name}' publishes a release outside the gate")
                    if re.search(r"\bgit\s+push\b[^\n]*\b(--tags|refs/tags|tag)\b", run):
                        problems.append(f"{path.name}: job '{job_name}' pushes a tag outside the gate")
    ruleset_file = repo_root / RULESET_PATH
    if not ruleset_file.exists():
        problems.append(f"{RULESET_PATH} is missing")
    else:
        problems += ruleset_problems(json.loads(ruleset_file.read_text()))
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(prog="gate.py", description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check-policy")
    p_scan = sub.add_parser("scan")
    p_scan.add_argument("--denylist-env")
    p_scan.add_argument("--require-denylist", action="store_true")
    p_scan.add_argument("paths", nargs="+")
    p_self = sub.add_parser("self-test")
    p_self.add_argument("--denylist-env")
    p_self.add_argument("--require-denylist", action="store_true")
    p_build = sub.add_parser("build-manifest")
    p_build.add_argument("directory")
    p_build.add_argument("--tag", required=True)
    p_build.add_argument("--sha", required=True)
    p_build.add_argument("--extra-json", help="JSON object merged into the manifest (e.g. image digests)")
    p_verify = sub.add_parser("verify-manifest")
    p_verify.add_argument("directory")
    p_verify.add_argument("--expect-digest")
    p_live = sub.add_parser("check-live-ruleset")
    p_live.add_argument("--repo", required=True)
    p_live.add_argument("--app-id", required=True)
    args = parser.parse_args(argv)

    if args.cmd == "check-policy":
        problems = check_policy()
        for p in problems:
            print(f"::error::{p}")
        print("policy ok" if not problems else f"{len(problems)} policy problem(s)")
        return 1 if problems else 0

    if args.cmd in ("scan", "self-test"):
        denylist = denylist_from_env(args.denylist_env) if args.denylist_env else []
        if args.require_denylist and not denylist:
            print(f"::error::the denylist ({args.denylist_env or 'none given'}) is empty; refusing to scan without it")
            return 1
        if args.cmd == "self-test":
            ok = self_test(denylist)
            print("scanner self-test passed" if ok else "::error::scanner self-test failed: a planted leak was not rejected")
            return 0 if ok else 1
        findings = scan_paths(args.paths, denylist=denylist, allowlist=load_allowlist())
        for f in findings:
            print(f"::error::{f}")
        print("scan clean" if not findings else f"{len(findings)} finding(s); nothing may be published")
        return 1 if findings else 0

    if args.cmd == "build-manifest":
        extra = json.loads(args.extra_json) if args.extra_json else None
        manifest = build_manifest(args.directory, args.tag, args.sha, extra)
        (pathlib.Path(args.directory) / MANIFEST_NAME).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        print(manifest_digest(manifest))
        return 0

    if args.cmd == "verify-manifest":
        manifest = json.loads((pathlib.Path(args.directory) / MANIFEST_NAME).read_text())
        problems = verify_manifest(args.directory, manifest)
        digest = manifest_digest(manifest)
        if args.expect_digest and digest != args.expect_digest:
            problems.append("manifest digest differs from the one the scan job approved")
        for p in problems:
            print(f"::error::{p}")
        print("candidate matches the scanned manifest" if not problems else "candidate does not match; refusing to publish")
        return 1 if problems else 0

    if args.cmd == "check-live-ruleset":
        token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or ""
        problems = live_ruleset_problems(fetch_live_rulesets(args.repo, token), args.app_id)
        for p in problems:
            print(f"::error::{p}")
        print("live tag ruleset ok" if not problems else "live tag ruleset does not enforce the gate; refusing to publish")
        return 1 if problems else 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
