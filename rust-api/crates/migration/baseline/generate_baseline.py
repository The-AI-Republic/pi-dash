#!/usr/bin/env python3
"""Emit the BASELINE const for rust-api/crates/migration/src/baseline.rs.

Walks the migration files on disk for every INSTALLED_APPS app that ships
migrations (first-party pi_dash apps plus Django contrib, DRF authtoken and
django_celery_beat) and prints the Rust const in dependency-neutral order
(grouped by app; the migrator treats the baseline as one set, not a chain).

Usage (from the repo root):
    python3 rust-api/crates/migration/baseline/generate_baseline.py \\
        > rust-api/crates/migration/src/baseline.rs

Then re-verify against a live migrate before cutting a new baseline:
    SELECT app, name FROM django_migrations ORDER BY app, name;
must match this set exactly (NEWPIDASH-380 workpad).
"""

import os

import django

DROOT = os.path.dirname(django.__file__)
# First-party apps, in INSTALLED_APPS order (apps/api/pi_dash/settings/common.py).
FIRST_PARTY = ["db", "runner", "license", "prompting", "assistant"]
# (site-packages path under the Django root, Django app label).
CONTRIB = [
    ("contrib/auth", "auth"),
    ("contrib/contenttypes", "contenttypes"),
    ("contrib/sessions", "sessions"),
]


def walk(d, label, entries):
    migrations = sorted(
        f[:-3] for f in os.listdir(d) if f.endswith(".py") and f != "__init__.py"
    )
    for name in migrations:
        entries.append((label, name))


def main():
    entries = []
    for app in FIRST_PARTY:
        walk(os.path.join("apps/api/pi_dash", app, "migrations"), app, entries)
    for pkg, label in CONTRIB:
        walk(os.path.join(DROOT, pkg, "migrations"), label, entries)
    import rest_framework
    import django_celery_beat

    for mod, label in [
        (rest_framework, "authtoken"),
        (django_celery_beat, "django_celery_beat"),
    ]:
        base = os.path.dirname(mod.__file__)
        for dirpath, _dirnames, filenames in os.walk(base):
            if os.path.basename(dirpath) == "migrations":
                for name in sorted(filenames):
                    if name.endswith(".py") and name != "__init__.py":
                        entries.append((label, name[:-3]))
    print("//! Baseline migration set: every Django migration at the leaf the")
    print("//! snapshot was cut at (`db/0183`, `runner/0043`; rebased for")
    print("//! `runner/0044` once NEWPIDASH-372 merges).")
    print("//!")
    print("//! Generated — do not hand-edit. Re-generate with:")
    print("//! `python3 rust-api/crates/migration/baseline/generate_baseline.py`,")
    print("//! run from the repo root, and re-verify against a live")
    print("//! `manage.py migrate` (`SELECT app, name FROM django_migrations`)")
    print("//! before cutting a new baseline.")
    print("")
    print(f"// {len(entries)} Django migrations at leaf.")
    print("pub const BASELINE: &[(&str, &str)] = &[")
    for app, name in entries:
        print(f'    ("{app}", "{name}"),')
    print("];")


if __name__ == "__main__":
    main()
