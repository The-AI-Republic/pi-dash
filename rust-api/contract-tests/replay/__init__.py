# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Write-replay validation (PIDASHCONV-823, stage 8 V-04).

Proves HTTP writes have the same effect on both backends. Each corpus
session restores one seeded snapshot into two databases, replays the
same ordered requests against Django (one copy) and Rust with all
cutover flags on (the other copy), and compares after every request:

- status and body (byte-equal, else JSON-equal after the volatile
  normalisation below — the same rules as the shadow-read comparer in
  ``crates/api/src/edge_shadow.rs``),
- the full database state per table, column by column, modulo the
  explicit ignore maps in :mod:`replay.effects` (generated ids and
  ``now()`` timestamps are named there, never auto-detected),
- the jobs enqueued (name + payload, ids dropped, ETA within tolerance),
- mail and webhook sink deliveries.

Layout: ``runner.py`` (engine), ``session.py`` (corpus model),
``snapshot.py`` (restore/clone/queue DDL), ``compare.py`` (comparers),
``effects.py`` (explicit ignore maps), ``coverage.py`` (route-coverage
CLI, not a pytest module), ``corpus/`` (sessions + world seed),
``test_replay.py`` (pytest driver), ``test_meta.py`` (drift guards that
need no live backends).
"""
