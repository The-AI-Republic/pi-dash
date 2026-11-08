# Binaries bundled with Pi Dash Desktop

`bin/` is populated at build time and is **not** committed — locally by
`scripts/prepare-agent.sh`, in release pipelines by `scripts/bundle_agents.py`:

| File     | Source                                                 | License       |
| -------- | ------------------------------------------------------ | ------------- |
| `pidash` | `The-AI-Republic/pi-dash`, tag `PIDASH_BUNDLE_VERSION` | AGPL-3.0-only |

The build also drops the project's `LICENSE` into this directory; the About
dialog renders it. It opens from the info button beside the sidebar's user
menu and reads these files through the `desktop_about` command
(`src/about.rs`). A new file staged here must be added to that module's
list, or it ships without being shown.

The built-in agent engine is not bundled: it ships inside the `pidash` CLI
release under `engine/<version>/` (with its own `LICENSE` and verbatim
upstream `NOTICE`) and is installed by `pidash engine install`. The
Apache-2.0 §4 obligation for the engine is met by the CLI release, which
retains the licence and notice and discloses the rename in
`engine/<version>/README.txt`.
