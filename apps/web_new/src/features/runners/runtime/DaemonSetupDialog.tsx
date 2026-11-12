// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// "Set up Pi Dash on this computer" (PIDESKAPP-222; design §4.7, PO-12).
// The consent screen the runtime service opens when it finds no daemon.
// The background service is required, so the screen says so; declining
// sets nothing up. Accepting runs the command-line tool's own set-up steps
// and shows which one is running, then hands control back to the chat or
// run that asked.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { agentFailureMessage } from "../../../shared/lib/agentReasons.js";
import { setUpDaemon, type DaemonSetUpStep } from "./service.js";
import { registerSetUpPrompt, type SetUpRequest } from "./setupPrompt.js";

export const SETUP_TITLE = "Set up Pi Dash on this computer?";

const REASON_TEXT: Record<SetUpRequest["reason"], string> = {
  chat: "Chatting with the built-in engine needs Pi Dash on this computer.",
  engine: "Running work on this computer needs Pi Dash on this computer.",
};

export const SETUP_BODY =
  "The app installs the Pi Dash command-line tool, signs it in to this workspace with your account, and starts it as a background service that keeps running when the app is closed. Nothing is set up unless you continue.";

export const SETUP_STEP_TEXT: Record<DaemonSetUpStep, string> = {
  install_cli: "Installing the command-line tool…",
  sign_in: "Signing it in…",
  service: "Starting the background service…",
  engine: "Installing the built-in engine…",
  done: "Pi Dash is set up on this computer.",
};

type Phase = { kind: "ask" } | { kind: "running"; step: DaemonSetUpStep } | { kind: "failed"; message: string };

export function DaemonSetupDialog(): React.ReactElement {
  const [request, setRequest] = React.useState<SetUpRequest | null>(null);
  const [phase, setPhase] = React.useState<Phase>({ kind: "ask" });
  const pending = React.useRef<((ok: boolean) => void) | null>(null);

  const settle = React.useCallback((ok: boolean): void => {
    const done = pending.current;
    pending.current = null;
    setRequest(null);
    setPhase({ kind: "ask" });
    if (done) done(ok);
  }, []);

  React.useEffect(() => {
    registerSetUpPrompt((next, done) => {
      // A second request while one is open joins it: both settle together.
      const previous = pending.current;
      pending.current = previous
        ? (ok) => {
            previous(ok);
            done(ok);
          }
        : done;
      setRequest((current) => current ?? next);
    });
    return () => {
      registerSetUpPrompt(null);
      settle(false);
    };
  }, [settle]);

  const running = phase.kind === "running";

  async function accept(): Promise<void> {
    if (!request) return;
    setPhase({ kind: "running", step: "install_cli" });
    try {
      await setUpDaemon({
        client: getAppClient(),
        runtime: platform.agentRuntime,
        cli: platform.cliSetup,
        workspaceSlug: request.workspaceSlug,
        onStep: (step) => setPhase({ kind: "running", step }),
      });
      settle(true);
    } catch (error) {
      setPhase({ kind: "failed", message: agentFailureMessage(error) });
    }
  }

  return (
    <Dialog
      title={SETUP_TITLE}
      description={request ? REASON_TEXT[request.reason] : undefined}
      open={request !== null}
      nonDismissable={running}
      onOpenChange={() => {
        if (!running) settle(false);
      }}
      actions={
        <>
          <Button variant="ghost" disabled={running} onClick={() => settle(false)}>
            Not now
          </Button>
          <Button variant="primary" disabled={running} onClick={() => void accept()} data-testid="daemon-setup-accept">
            {phase.kind === "failed" ? "Try again" : "Set up"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-3)" data-testid="daemon-setup-dialog">
        <p className="text-body text-(--text)">{SETUP_BODY}</p>
        {phase.kind === "running" ? (
          <p role="status" className="text-caption text-(--text-muted)">
            {SETUP_STEP_TEXT[phase.step]}
          </p>
        ) : null}
        {phase.kind === "failed" ? (
          <p role="alert" className="text-caption text-(--danger)">
            {phase.message}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
