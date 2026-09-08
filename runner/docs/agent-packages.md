# Bring your own agent package

OSS provides an opt-in integration seam, not a bundled agent product. A
developer supplies a trusted local executable (or executable adapter), then
connects it to an already-enrolled runner. Pi Dash does **not** download,
install, license, authenticate, configure the model provider for, or update
that package. Existing runners are unchanged unless explicitly configured.

## Package contract

Create a local UTF-8 TOML manifest alongside your package:

```toml
version = 1
protocol = "codex"
executable = "bin/my-task-engine"
```

On Windows, point `executable` at the actual `.exe` entry point, for example
`"bin/my-task-engine.exe"`. Paths may be absolute or relative to the manifest's
directory; relative paths are resolved to an absolute path at configuration
time. Spaces are supported. The entry point must already exist, be a file,
and have executable permission on Unix. It is a path, **not** a shell command,
download URL, package-manager reference, or argument list.

`version`, `protocol`, and `executable` are required. Unknown fields, unsupported
versions/protocols, and missing executables fail without changing runner config.
There are no install hooks, dependency resolution, or manifest secret fields.
Treat the executable and manifest as trusted local code: this is not a security
boundary or a mechanism for accepting packages from issues or remote users.

The protocol selects an existing bridge's complete CLI and wire contract:

| `protocol`     | Contract implemented by the supplied executable                                   |
| -------------- | --------------------------------------------------------------------------------- |
| `codex`        | `app-server`: Codex JSON-RPC initialization, threads, turns, events and approvals |
| `claude_code`  | Claude Code headless stream-JSON CLI, including session/resume arguments          |
| `cursor_agent` | Cursor Agent headless stream-JSON CLI, including session/resume arguments         |
| `open_claw`    | The **acpx client** CLI contract, including `--format json openclaw exec`         |
| `grok`         | `agent stdio`: the Grok bridge's ACP server contract                              |

These names identify protocols, not a restriction on who supplies the binary.
An arbitrary executable that prints text is not protocol-compatible. To use a
different protocol, implement an executable adapter that translates one of
these contracts, or add a bridge to `runner/src/agent/`. Setting a new name in
the manifest does not dynamically load a Rust plugin. The existing bridge
implementations and their fake-agent integration tests are the contract source.

## Connect an existing runner

After the operator has enrolled a runner through the normal Pi Dash workflow:

```bash
pidash runner use-package my-runner --manifest /path/to/package/agent-package.toml
```

This command only updates the named runner's agent kind and executable using
the existing locked, validated, atomic configuration writer. Other runners,
Pi Dash credentials, cloud registration, working directory, model settings,
and approval policy are preserved. The executable is not started or probed.
Reapplying the manifest is safe and updates its resolved executable path.

Restart the **owning daemon when idle** to apply the change; the command does
not restart it or interrupt an active task. For an operator-managed service,
use the normal `pidash restart` workflow. An embedded host manages its own
lifecycle. If the package moves, reapply the manifest before restarting.

Runs use the normal runner execution path, including existing approvals,
cancellation, status reporting, and workspace handling. An ordinary task folder
does not need Git; explicit repository clones and worktree pools retain their
Git requirements. No `managed_runner` feature flag, desktop session, OpenHub
account, or private-repository component is required for this local-runner seam.

The package owner must supply runtime dependencies, model configuration and
credentials through the agent's own supported mechanism. A local executable
wrapper can establish those settings and then forward the bridge's arguments
and stdio unchanged. Do not embed secrets in the manifest or argv.

## Rust integration seam

Hosts linking the OSS runner crate may use the same implementation:

```rust,ignore
use pidash::agent::package::AgentPackage;
use pidash::config::file;

let package = AgentPackage::from_manifest(manifest_path)?;
file::mutate_config(paths, |config| {
    let runner = config.runners.iter_mut()
        .find(|runner| runner.name == "my-runner")
        .ok_or_else(|| anyhow::anyhow!("runner not found"))?;
    package.apply_to_runner(runner)
})?;
```

The seam refuses to overwrite runners carrying the private desktop host's
managed Codex environment fields. That host owns its package and credentials;
its bundle integration remains separate. Developers integrating their own
packages should use their own runner configuration, not repurpose a desktop
runner's credentials.
