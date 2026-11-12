/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { Button } from "@pi-dash/propel/button";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { registerSetUpPrompt, setUpCliForThisMachine, type SetUpReason, type SetUpStep } from "@/services/pidash-cli";

interface TauriEvents {
  event: { listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<() => void> };
}

const REASON_TEXT: Record<SetUpReason, string> = {
  engine: "to run work on this computer with the built-in engine",
  runner: "to add a runner on this computer",
  chat: "to chat with the built-in engine on this computer",
};

const STEP_TEXT: Record<SetUpStep, string> = {
  install_cli: "Installing the Pi Dash CLI…",
  sign_in: "Signing it in with your account…",
  service: "Installing and starting the background service…",
  engine: "Installing the built-in engine…",
  done: "Done.",
};

/**
 * "Set up Pi Dash on this computer" (design §4.7, §4.8; PIDESKAPP-114).
 *
 * The consent screen says what the set-up does before anything happens: Pi
 * Dash runs as a background service, which keeps this computer's runners —
 * the built-in engine included — available while the app is closed; how to
 * pause it; how to remove it. The service is required (PO-12): "Not now" sets
 * nothing up, and the app keeps working as a cloud client and offers set-up
 * again next time.
 *
 * The progress log is the CLI's own output, rendered as plain text only — never
 * as HTML (see `pidash_cli.rs`).
 */
export function DesktopDaemonSetupDialog() {
  const [request, setRequest] = useState<{ reason: SetUpReason; workspaceSlug: string } | null>(null);
  const [step, setStep] = useState<SetUpStep | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const done = useRef<((ok: boolean) => void) | null>(null);

  useEffect(() => {
    registerSetUpPrompt((next, resolve) => {
      done.current?.(false);
      done.current = resolve;
      setRequest(next);
      setStep(null);
      setError(null);
      setLog([]);
    });
    return () => {
      registerSetUpPrompt(null);
      done.current?.(false);
      done.current = null;
    };
  }, []);

  useEffect(() => {
    if (!request) return;
    const tauri = (window as unknown as { __TAURI__?: TauriEvents }).__TAURI__;
    let unlisten: (() => void) | undefined;
    let closed = false;
    void tauri?.event
      ?.listen<{ line?: string }>("pidash-install-log", (e) => {
        const line = e.payload?.line;
        if (typeof line === "string") setLog((lines) => [...lines.slice(-199), line]);
      })
      .then((stop) => {
        // Closed before the listener was registered: drop it at once.
        if (closed) stop();
        else unlisten = stop;
        return stop;
      });
    return () => {
      closed = true;
      unlisten?.();
    };
  }, [request]);

  const finish = (ok: boolean) => {
    done.current?.(ok);
    done.current = null;
    setRequest(null);
  };

  const start = async () => {
    if (!request) return;
    setError(null);
    try {
      await setUpCliForThisMachine(request.workspaceSlug, setStep);
      finish(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const running = step !== null && step !== "done" && !error;
  const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

  return (
    <ModalCore
      isOpen={request !== null}
      handleClose={() => !running && finish(false)}
      position={EModalPosition.CENTER}
      width={EModalWidth.XXL}
    >
      <div data-prevent-outside-click className="flex flex-col gap-4 p-5">
        <h3 className="text-16 font-medium text-primary">Set up Pi Dash on this computer</h3>
        {request && (
          <p className="text-13 text-secondary">
            Pi Dash needs to be set up on this computer {REASON_TEXT[request.reason]}.
          </p>
        )}
        <ul className="list-disc space-y-1 pl-5 text-13 text-secondary">
          <li>
            Pi Dash runs as a background service on this computer. It keeps your runners, the built-in engine included,
            available when this app is closed.
          </li>
          <li>It signs in with the account you use in this app. No second sign-in.</li>
          <li>
            You can pause the built-in engine at any time in Built-in engine settings, or stop the service with{" "}
            <code>pidash stop</code>. To remove it, run <code>pidash uninstall</code>.
          </li>
          {isMac && <li>macOS shows a “background item added” notice for the service.</li>}
          <li>Without the service there is no built-in engine, local chat or own-agent runner on this computer.</li>
        </ul>
        {step && <p className="text-13 text-primary">{STEP_TEXT[step]}</p>}
        {log.length > 0 && (
          <pre className="max-h-48 overflow-auto rounded-md border border-subtle bg-layer-1 p-3 text-11 whitespace-pre-wrap text-secondary">
            {log.join("\n")}
          </pre>
        )}
        {error && <p className="text-13 text-danger-primary">Set-up did not finish: {error}</p>}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={() => finish(false)} disabled={running}>
            {error ? "Close" : "Not now"}
          </Button>
          <Button onClick={() => void start()} loading={running} disabled={running}>
            {error ? "Try again" : "Set up"}
          </Button>
        </div>
      </div>
    </ModalCore>
  );
}
