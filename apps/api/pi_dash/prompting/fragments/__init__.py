# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Modular prompt fragments.

The default agent prompt is assembled from ordered markdown+Jinja fragments in
this directory. ``assemble()`` concatenates them into a single template body
which is then stored in the ``PromptTemplate`` DB row and rendered via the
sandboxed Jinja env at turn-start time.

Fragment filenames use a ``NN_name.md`` convention so lexical sort yields the
correct order. Fragments are markdown with embedded Jinja2 markers; the ``.md``
extension keeps GitHub preview working. Trimming + ``"\\n\\n".join(...)`` keeps
the assembled body tidy regardless of per-fragment trailing blank lines.

Assembly happens at seed time, not at render time — the sandboxed renderer
does not have a filesystem loader, which keeps ``{% include %}`` out of the
attack surface for workspace-admin-editable templates.
"""

from __future__ import annotations

from pathlib import Path

FRAGMENTS_DIR = Path(__file__).resolve().parent

# Match only files that follow the NN_name.md numeric-prefix convention so a
# stray README.md / NOTES.md in this directory cannot silently get spliced
# into the production prompt.
FRAGMENT_GLOB = "[0-9][0-9]_*.md"


def fragment_paths() -> list[Path]:
    """Return fragment files in assembly order (lexical)."""
    return sorted(FRAGMENTS_DIR.glob(FRAGMENT_GLOB))


def assemble() -> str:
    """Read every fragment and return the concatenated template body."""
    parts = [p.read_text(encoding="utf-8").strip() for p in fragment_paths()]
    return "\n\n".join(parts) + "\n"
