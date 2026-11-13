#!/usr/bin/env python3
"""Ratchet: fail when production Rust code mentions a legacy github_* table/model.

NEWPIDASH-210 (github_* sunset): the Rust backend must read/write only the
generic git_* tables. The legacy tables (github_repositories,
github_repository_syncs, github_issue_syncs, github_comment_syncs,
github_app_installations, github_app_install_sessions,
github_webhook_deliveries, github_pull_request_links) and their Github*
model types stay in the tree only as entity definitions under
crates/db/src/entity/ until the drop-tables follow-up (PDASHOSS01-353).

This script scans rust-api/crates and rust-api/bin for *.rs and fails when a
production mention of a legacy table or model appears outside the exclusions
below. "Production" means test code (#[cfg(test)] items and their sibling
files) and comments are masked out, the same way write_guard.py lexes; string
contents stay visible, so a raw-SQL literal naming a legacy table still fails.

Two coordination lists live in this file:

* EXCLUDED_358 -- files owned by PDASHOSS01-358 (PR #3041), which deletes
  the legacy poller and stops project-bind legacy writes. They are skipped
  entirely while that PR is in flight; when it lands and cleans them, the
  guard prints a notice so a follow-up can drop the entries. Never add to
  this list to silence a hit -- the file belongs here only because 358
  edits it in the same window.

* scripts/github_sunset_allowlist.txt -- files that keep a legacy mention on
  purpose, one per line as `path | reason`. A file with hits that is not on
  the list fails; a list entry with no hits fails too, so the list only
  shrinks. Current entries: the cascade-deletion sweep (which must still
  delete legacy rows until the drop-tables follow-up), the OpenAPI schema
  name (wire compatibility, not table access), and the model-definition
  files slated for deletion in NEWPIDASH-210 part 3 (after PDASHOSS01-358
  merges).

Entity definitions (crates/db/src/entity/) and anything under a migrations/
directory are never scanned.

Usage:
    python3 scripts/github_sunset_guard.py --check   # exit 0 iff hits == allowlist
    python3 scripts/github_sunset_guard.py --seed    # print today's hits

Pure stdlib, no DB. Runs in CI (rust-api-github-sunset-guard.yml).
"""

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from write_guard import lex, mask_cfg_test  # noqa: E402

LEGACY_TABLES = (
    "github_repositories",
    "github_repository_syncs",
    "github_issue_syncs",
    "github_comment_syncs",
    "github_app_installations",
    "github_app_install_sessions",
    "github_webhook_deliveries",
    "github_pull_request_links",
)

LEGACY_MODELS = (
    "GithubRepository",
    "GithubRepositorySync",
    "GithubIssueSync",
    "GithubCommentSync",
    "GithubAppInstallation",
    "GithubWebhookDelivery",
    "GithubPullRequestLink",
    "GithubAccountType",
    "GithubRepositorySelection",
    "GithubSessionStatus",
    "GithubWebhookStatus",
    "UnknownGithubAccountType",
    "UnknownGithubRepositorySelection",
    "UnknownGithubWebhookStatus",
    "UnknownGithubSessionStatus",
    "GithubRepositories",
    "GithubRepositorySyncs",
    "GithubIssueSyncs",
    "GithubCommentSyncs",
    "GithubAppInstallations",
    "GithubWebhookDeliveries",
    "GithubPullRequestLinks",
)

LEGACY_MODULES = (
    "github_repository",
    "github_repository_sync",
    "github_issue_sync",
    "github_comment_sync",
    "github_webhook_delivery",
    "github_pull_request_link",
    "github_models",
)

PATTERN = re.compile(
    r"\b(?:%s)\b" % "|".join(
        sorted([*LEGACY_TABLES, *LEGACY_MODELS, *LEGACY_MODULES], key=len, reverse=True)
    )
)

# Files owned by PDASHOSS01-358 (PR #3041). Skipped, not allowlisted: that
# PR removes their legacy hits, and a stale allowlist entry would fail on
# main at its merge. Remove each entry here when the file no longer has
# legacy hits (the guard warns until then).
EXCLUDED_358 = frozenset({
    "crates/api/src/app_integrations/handlers_git_repo.rs",
    "crates/api/src/app_integrations/handlers_github_proj.rs",
    "crates/jobs/src/integrations/github_sync.rs",
    "crates/services/src/integrations/code_reviews.rs",
    "crates/services/src/integrations/repositories.rs",
})

ENTITY_PREFIX = "crates/db/src/entity/"
SKIP_SEGMENT = "/migrations/"
SCAN_ROOTS = ("crates", "bin")


def iter_rs_files(root):
    for top in SCAN_ROOTS:
        base = root / top
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if rel.startswith(ENTITY_PREFIX):
                continue
            if SKIP_SEGMENT in "/" + rel + "/":
                continue
            yield path, rel


def collect_hits(root):
    hits = {}  # rel -> list of "rel:line: match"
    clean_excluded = []
    skip_files = set()
    files = []
    for path, rel in iter_rs_files(root):
        src = path.read_text(encoding="utf-8")
        code, spans = lex(src)
        masked, file_mods = mask_cfg_test(code, spans)
        for mod in file_mods:
            skip_files.add(path.parent / (mod + ".rs"))
            skip_files.add(path.parent / mod / "mod.rs")
        files.append((path, rel, masked))
    for path, rel, masked in files:
        if path in skip_files:
            continue
        if rel in EXCLUDED_358:
            continue
        for m in PATTERN.finditer(masked):
            line = masked.count("\n", 0, m.start()) + 1
            hits.setdefault(rel, []).append("%s:%d: %s" % (rel, line, m.group(0)))
    # Notice for excluded files that are already clean: a follow-up can
    # shrink EXCLUDED_358. Never fails the check.
    for rel in sorted(EXCLUDED_358):
        target = root / rel
        if not target.exists():
            clean_excluded.append("%s (missing)" % rel)
            continue
        src = target.read_text(encoding="utf-8")
        code, spans = lex(src)
        masked, _ = mask_cfg_test(code, spans)
        if not PATTERN.search(masked):
            clean_excluded.append(rel)
    return hits, clean_excluded


def read_allowlist(path):
    entries = {}
    if not path.exists():
        return entries
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if "|" not in line:
            print("error: malformed allowlist line: %r" % raw, file=sys.stderr)
            sys.exit(2)
        rel, reason = line.split("|", 1)
        entries[rel.strip()] = reason.strip()
    return entries


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", default=None,
                    help="rust-api directory (default: script's parent)")
    ap.add_argument("--allowlist", default=None,
                    help="allowlist path (default: <root>/scripts/...)")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--seed", action="store_true")
    args = ap.parse_args(argv)

    root = Path(args.root) if args.root else Path(__file__).resolve().parent.parent
    allow_path = (Path(args.allowlist) if args.allowlist
                  else root / "scripts" / "github_sunset_allowlist.txt")
    hits, clean_excluded = collect_hits(root)

    if args.seed:
        for rel in sorted(hits):
            print("%s | justification" % rel)
        return 0

    allowed = read_allowlist(allow_path)
    new = sorted(set(hits) - set(allowed))
    stale = sorted(set(allowed) - set(hits))
    total = sum(len(v) for v in hits.values())
    print("github_sunset_guard: %d hit(s) in %d file(s), %d allowlisted"
          % (total, len(hits), len(allowed)))
    if clean_excluded:
        print("github_sunset_guard: excluded (PDASHOSS01-358) files already clean")
        for rel in clean_excluded:
            print("  notice: %s" % rel)
    if not new and not stale:
        print("github_sunset_guard: OK -- no unallowlisted legacy mentions")
        return 0
    for rel in new:
        for hit in hits[rel]:
            print("NEW MENTION: %s" % hit)
    for rel in stale:
        print("STALE ENTRY (remove from the allowlist): %s" % rel)
    return 1


if __name__ == "__main__":
    sys.exit(main())
