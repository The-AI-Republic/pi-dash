// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// In-app command-line setup entry (DESK-021, parity/specs/desktop.md §5).
// The panel is the page side of the native tool: it reports the installed
// version, runs one setup at a time, shows the installer's output lines as
// plain text in arrival order, and finishes by signing the tool in with a
// grant this signed-in app starts and approves itself. Builds without the
// native tool capability render nothing at all.

import { Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/index.js";
import { platform } from "../../../core/platform/index.js";
import type { CliInstallProgress, CliStatus } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { createSetupLock, setUpCli, type CliSetupStage } from "../cliSetup.js";

export const CLI_SETUP_HEADING = "Command-line tool";
export const CLI_SETUP_INTRO =
  "Install the bundled pidash tool and sign it in to this workspace, so this machine can host a runner.";
const CLI_SETUP_DONE = "The command-line tool is ready.";
const CLI_SETUP_FAILED_TITLE = "Couldn't set up the command-line tool";

type SetupPhase = "idle" | "checking" | CliSetupStage | "done";

const STAGE_TEXT: Record<CliSetupStage, string> = {
  detecting: "Checking the tool…",
  installing: "Installing the tool…",
  "signing-in": "Signing the tool in…",
};

/** What the summary line reports about the tool. */
export function describeCliStatus(status: CliStatus | null, checking: boolean): string {
  if (checking) return "Checking this machine for the tool…";
  if (status === null) return "Not checked yet.";
  if (!status.installed) return "Not installed on this machine.";
  return status.version === null ? "Installed." : `Installed, version ${status.version}.`;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) return error.message;
  return "Something went wrong while setting up the tool.";
}

export interface CliSetupPanelProps {
  workspaceSlug: string;
  /** Address the tool talks to; defaults to the app's API base. */
  cloudUrl?: string;
}

export function CliSetupPanel({ workspaceSlug, cloudUrl }: CliSetupPanelProps): React.ReactElement | null {
  const cli = platform.cliSetup;
  const lock = React.useRef(createSetupLock());
  const [status, setStatus] = React.useState<CliStatus | null>(null);
  const [phase, setPhase] = React.useState<SetupPhase>("idle");
  const [progress, setProgress] = React.useState<readonly string[]>([]);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (cli === undefined) return undefined;
    let active = true;
    setPhase("checking");
    void cli.detect().then(
      (next) => {
        if (!active) return;
        setStatus(next);
        setPhase("idle");
      },
      (failure: unknown) => {
        if (!active) return;
        setPhase("idle");
        setError(errorMessage(failure));
      }
    );
    return () => {
      active = false;
    };
  }, [cli]);

  if (cli === undefined) return null;

  const busy = phase !== "idle" && phase !== "done";
  const installed = status?.installed === true;

  function reportProgress(entry: CliInstallProgress): void {
    setProgress((lines) => [...lines, entry.line]);
  }

  function reportStage(stage: CliSetupStage): void {
    setPhase(stage);
  }

  async function setUp(): Promise<void> {
    setError(null);
    setProgress([]);
    try {
      const client = getAppClient();
      const result = await setUpCli(
        { client, cloudUrl: cloudUrl ?? client.baseUrl, workspace: workspaceSlug, cli },
        lock.current,
        reportProgress,
        reportStage
      );
      setStatus(result.status);
      setPhase("done");
      notifySuccess(CLI_SETUP_DONE);
    } catch (failure) {
      const message = errorMessage(failure);
      setError(message);
      setPhase("idle");
      notifyError(CLI_SETUP_FAILED_TITLE, message);
    }
  }

  return (
    <section
      aria-label={CLI_SETUP_HEADING}
      data-testid="cli-setup-panel"
      className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h2 className="text-h4 text-(--text)">{CLI_SETUP_HEADING}</h2>
          <p className="text-body text-(--text-muted)">{CLI_SETUP_INTRO}</p>
          <p className="text-meta text-(--text-muted)" data-testid="cli-setup-status">
            {describeCliStatus(status, phase === "checking")}
          </p>
        </div>
        <Button loading={busy} onClick={() => void setUp()}>
          {installed ? "Sign in the tool" : "Set up the tool"}
        </Button>
      </div>
      {phase === "installing" || phase === "signing-in" ? (
        <p className="text-meta text-(--text-muted)" data-testid="cli-setup-stage">
          {STAGE_TEXT[phase]}
        </p>
      ) : null}
      {progress.length === 0 ? null : (
        <pre
          aria-label="Install output"
          data-testid="cli-setup-log"
          className="text-meta max-h-48 overflow-y-auto rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text)"
        >
          {progress.join("\n")}
        </pre>
      )}
      {error === null ? null : (
        <p role="alert" className="text-body text-(--danger)">
          {error}
        </p>
      )}
    </section>
  );
}
