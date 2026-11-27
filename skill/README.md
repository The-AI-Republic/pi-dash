# Pi Dash Skill

An agent skill that teaches Claude Code or Codex to use Pi Dash through the
`pidash` CLI — creating, searching, updating, and commenting on issues, linking
pull requests, and reading project pages without leaving the coding session.

The skill is one portable `SKILL.md` package in [`pidash/`](pidash/). It lives in
this repository next to the CLI it documents ([`runner/`](../runner/)), so a CLI
change and the matching skill change land in the same pull request.

## Prerequisites

Install and authenticate the Pi Dash CLI first:

```bash
pidash auth login
pidash workspace me
```

The skill uses `.pidash/context.md` in the current workspace to decide which Pi
Dash project to use. Initialize it when needed:

```bash
pidash context init --project <project-id-or-identifier>
```

## Install via npm (recommended)

No clone required:

```bash
npx @airepublic/pidash-skill-installer
```

The installer prompts for a target — **All** (Claude Code + Codex, default),
**Claude Code only**, or **Codex only** — and copies the skill into the agent's
skills directory. The skill ships inside the npm package, so the installer makes
no network requests of its own. Works identically on macOS, Linux, and Windows
(PowerShell or cmd).

Non-interactive use (required when stdin isn't a TTY, e.g. CI):

```bash
npx @airepublic/pidash-skill-installer --all
npx @airepublic/pidash-skill-installer --claude-code
npx @airepublic/pidash-skill-installer --codex
```

Requires Node.js 18+. Set `CLAUDE_HOME` or `CODEX_HOME` to override the default
install locations (`~/.claude` and `~/.codex`). Restart your agent after
installing.

## Install from a clone

For development or to test local changes:

```bash
git clone https://github.com/The-AI-Republic/pi-dash.git
cd pi-dash
node skill/install.mjs
```

Same prompts and flags as the npm path.

## Install in Codex with `$skill-installer`

Codex ships a system skill that installs other skills straight from a GitHub
repo. Inside a Codex session, ask:

```text
Install the pidash skill from github.com/The-AI-Republic/pi-dash at skill/pidash
```

It places the folder in `~/.codex/skills/pidash/` (or
`$CODEX_HOME/skills/pidash`). Restart Codex after installing.

## Manual install

From a clone of this repository, copy the skill folder yourself:

```bash
# Claude Code — the Codex-only agents/ manifest is not needed
mkdir -p ~/.claude/skills
rm -rf ~/.claude/skills/pidash
cp -R skill/pidash ~/.claude/skills/pidash
rm -rf ~/.claude/skills/pidash/agents

# Codex
mkdir -p ~/.codex/skills
rm -rf ~/.codex/skills/pidash
cp -R skill/pidash ~/.codex/skills/pidash
```

In Claude Code, invoke it directly with `/pidash create an issue to change the
font size`, or let it load automatically when the request is clearly about Pi
Dash issues, projects, or workspace context.

## Update

Each installer version carries the skill content it was published with, so
updating means running the latest installer:

```bash
npx @airepublic/pidash-skill-installer@latest
```

From a clone: `git pull`, then `node skill/install.mjs`.

The installer replaces the existing `pidash` skill folder, and removes a
`pi-dash` folder left behind by v0.1.0, when the skill had that name.

## Verify

In the project workspace where the agent will run:

```bash
pidash --version
pidash workspace me
pidash project list
```

Then ask the agent to create or inspect a Pi Dash issue. If
`.pidash/context.md` is missing, the agent should list projects, propose the
best match, ask for confirmation, and initialize workspace context after you
confirm.

## Layout

- `install.mjs` — the installer; published as `@airepublic/pidash-skill-installer`.
- `pidash/SKILL.md` — frontmatter plus the short behavior guide the agent loads
  when the skill activates.
- `pidash/references/pidash-workflows.md` — the full command reference that
  `SKILL.md` tells the agent to read before its first write.
- `pidash/agents/openai.yaml` — Codex interface manifest. Not copied into
  Claude Code installs.
- `test/` — installer and skill-content tests (`npm test`).

## Maintaining

When a pull request adds, renames, or changes an agent-facing `pidash` command
in `runner/src/cli/`, update `pidash/SKILL.md` and
`pidash/references/pidash-workflows.md` in the same pull request. `npm test`
checks that every `pidash` command and flag the skill mentions still exists in the CLI
source.

To publish: bump `version` in `package.json`, then run `npm publish` from this
directory. `npm pack --dry-run` shows exactly what will ship.

## History

This skill was developed in
[The-AI-Republic/pi-dash-skill](https://github.com/The-AI-Republic/pi-dash-skill)
up to installer v0.2.0. That repository is no longer updated; its commit
history stays there.
