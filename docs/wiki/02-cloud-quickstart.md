# 02 — Pi Dash Cloud Quickstart

## 1. Sign up

1. Open <https://pidash.airepublic.com>.
2. Click **Sign up**, complete sign-up, verify your email.

## 2. Create workspace + project

1. Create your workspace when prompted.
2. **Projects → + Add project**. Fill in:
   - **Name**, **Identifier**
   - **Repository URL** — git clone URL (e.g. `git@github.com:acme/web.git`)
   - **Default branch** — usually `main`
3. Open **Project → Settings → General** and copy the **project ID**.

## 3. Install an AI agent

Pick one:

- **Claude Code** — install per <https://docs.anthropic.com/en/docs/claude-code>, verify `claude --version`.
- **Codex** — install per <https://github.com/openai/codex>, verify `codex --version`.

## 4. Install `pidash`

**macOS / Linux:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex
```

When prompted, enter `https://pidash.airepublic.com` and approve the device-code in the browser.

## 5. Register this machine as a runner

```bash
pidash runner add --project <PROJECT_ID>
pidash doctor
pidash status
```

`pidash doctor` must be clean before continuing.

## 6. Run your first work item

1. In Pi Dash, **Work items → + Create work item**.
2. Write a specific title + description + acceptance criteria.
3. Click **Run agent**.
4. Watch in the work item's **Run** panel or `pidash tui`.
5. Approve prompts as they appear (use **Approve always** for safe commands).

## 7. Review

1. Open the work item → review **diff**, **transcript**, **summary**.
2. To commit: `cd` into the runner working dir (`pidash status` shows the path), then `git` as normal.

---

## Common commands

```bash
pidash status              # daemon + runner state
pidash tui                 # interactive dashboard
pidash doctor              # preflight checks
pidash auth status         # who am I logged in as
pidash runner list         # runners on this host
pidash runner add --project <ID>
pidash runner remove <name>
pidash restart / stop
pidash update --restart    # self-update and restart when idle
pidash --help
```

Full command + flag reference: [17 — `pidash` CLI reference](./17-cli-reference.md).

## If something breaks

- **Run stuck pending** → `pidash status`, then `pidash restart`.
- **Run fails immediately** → `pidash doctor` (usually agent not on `PATH`, or git clone auth).
- **Command not found** → open a new terminal.
- **Browser auth not approving** → use the same browser you signed up in.
- **Headless host** → use enrollment token: in web UI **Runners → Add connection**, then `pidash connect --url https://pidash.airepublic.com --token <TOKEN>`.

Discussions: <https://github.com/The-AI-Republic/pi-dash/discussions> · Bugs: <https://github.com/The-AI-Republic/pi-dash/issues>
