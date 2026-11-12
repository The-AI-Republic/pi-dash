// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// "Is this <label>, last seen <date>? Replace it" (design §4.10), shown when
// Pi Dash on this computer signed in as a new machine and the user has an
// offline one with the same hostname. Closing without choosing decides
// nothing: the app asks again on its next launch.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import {
  declineReplace,
  errorText,
  registerReplacePrompt,
  replaceMachine,
  setReplacePromptOpen,
  type ReplaceOutcome,
  type ReplacePrompt,
} from "./desktop-replace.js";
import { compatibleHello, currentDiscovery, signInWorkspace } from "./service.js";

function lastSeen(at: string | null): string {
  if (!at) return "never";
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "never" : date.toLocaleDateString();
}

function resultText(outcome: ReplaceOutcome): string {
  const parts: string[] = [];
  if (outcome.restored.length) parts.push(`The built-in engine is back on for ${outcome.restored.join(", ")}.`);
  if (outcome.ownAgentRunners) {
    parts.push(
      `${outcome.ownAgentRunners} runner${outcome.ownAgentRunners === 1 ? "" : "s"} with your own agent stay offline until you set them up again on this computer or remove them.`
    );
  }
  return parts.join(" ") || "Done.";
}

export function ReplaceMachineDialog(): React.ReactElement {
  const [state, setState] = React.useState<{ account: string; prompt: ReplacePrompt } | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [outcome, setOutcome] = React.useState<ReplaceOutcome | null>(null);

  React.useEffect(() => {
    registerReplacePrompt((next) => {
      setState(next);
      setBusy(false);
      setError(null);
      setOutcome(null);
    });
    return () => registerReplacePrompt(null);
  }, []);

  React.useEffect(() => {
    setReplacePromptOpen(state !== null);
  }, [state]);
  React.useEffect(() => () => setReplacePromptOpen(false), []);

  const close = (): void => {
    if (!busy) setState(null);
  };

  const run = async (action: () => Promise<void>): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (caught) {
      setError(errorText(caught));
    } finally {
      setBusy(false);
    }
  };

  const confirm = (): void => {
    void run(async () => {
      if (!state) return;
      const runtime = platform.agentRuntime;
      if (!runtime) throw new Error("Pi Dash is not available on this build.");
      const hello = compatibleHello(currentDiscovery());
      if (!hello) throw new Error("Pi Dash on this computer is not connected right now. Try again in a moment.");
      const client = getAppClient();
      const deps = {
        runtime,
        client,
        cli: platform.cliSetup,
        cloudUrl: platform.apiOrigin ?? client.baseUrl,
        discovery: () => currentDiscovery(),
        signInWorkspace: (workspaceSlug: string) =>
          signInWorkspace({ client, runtime, cli: platform.cliSetup, workspaceSlug }),
      };
      setOutcome(await replaceMachine(state.prompt, deps));
    });
  };

  const decline = (): void => {
    void run(async () => {
      if (!state) return;
      const runtime = platform.agentRuntime;
      if (!runtime) throw new Error("Pi Dash is not available on this build.");
      await declineReplace(state.account, state.prompt, { runtime });
      setState(null);
    });
  };

  const candidate = state?.prompt.candidate;
  const name = candidate ? candidate.label || candidate.display_name || candidate.host_label : "";

  return (
    <Dialog
      title={`Is this ${name}, last seen ${lastSeen(candidate?.last_seen_at ?? null)}?`}
      open={state !== null}
      onOpenChange={(open) => {
        if (!open) close();
      }}
      actions={
        outcome ? (
          <Button onClick={() => setState(null)}>Close</Button>
        ) : (
          <>
            <Button variant="ghost" onClick={decline} disabled={busy}>
              No, keep it
            </Button>
            <Button variant="primary" onClick={confirm} disabled={busy}>
              Replace it
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-(--space-4)" data-testid="replace-machine-dialog">
        {outcome ? (
          <p className="text-body text-(--text)">
            {name} was replaced by this computer. {resultText(outcome)}
          </p>
        ) : (
          <p className="text-body text-(--text)">
            Pi Dash on this computer signed in as a new machine. If this is the same computer as {name}, replace it: its
            runners, pins and pods move here, and the built-in engine picks up its projects again. If it is a different
            computer, keep it.
          </p>
        )}
        {outcome?.failed.length ? (
          <p className="text-caption text-(--danger)">
            Could not turn the built-in engine back on for{" "}
            {outcome.failed.map((f) => `${f.project} (${f.error})`).join(", ")}. Turn it on from Built-in engine
            settings.
          </p>
        ) : null}
        {error ? <p className="text-caption text-(--danger)">Replacing it did not finish: {error}</p> : null}
      </div>
    </Dialog>
  );
}
