# Third-party binaries bundled with Pi Dash Desktop

`bin/` is populated at build time and is **not** committed — locally by
`scripts/prepare-agent.sh`, in release pipelines by `scripts/bundle_agents.py`:

| File | Source | License |
|---|---|---|
| `pidash` | `The-AI-Republic/pi-dash`, tag `PIDASH_BUNDLE_VERSION` | AGPL-3.0-only |
| `pidash-agent-engine` | upstream `openai/codex` release asset, tag `CODEX_BUNDLE_VERSION`, unmodified bytes, renamed | Apache-2.0 |

The build also drops each project's `LICENSE` (and `NOTICE`, where one exists) into
this directory; the About dialog renders them. It opens from the info button
beside the sidebar's user menu and reads these files through the `desktop_about`
command (`src/about.rs`). A new file staged here must be added to that module's
list, or it ships without being shown.

Two obligations to keep in mind when changing this:

- **Apache-2.0 §4** requires retaining the licence and NOTICE, and stating
  that the file was modified. Renaming counts, so the About dialog says so.
- **Apache-2.0 §6** grants no trademark rights: the engine is never called
  "Codex" in user-facing copy. Internally the name is fine.
