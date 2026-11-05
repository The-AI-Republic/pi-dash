/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { Button } from "@pi-dash/propel/button";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import {
  declineReplace,
  registerReplacePrompt,
  replaceMachine,
  type ReplaceOutcome,
  type ReplacePrompt,
} from "@/services/desktop-replace";

function lastSeen(at: string | null): string {
  if (!at) return "never";
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "never" : date.toLocaleDateString();
}

function message(error: unknown): string {
  if (error instanceof Error) return error.message;
  const data = (error as { error?: string } | null) ?? null;
  return data?.error ?? String(error);
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

/**
 * "Is this <label>, last seen <date>? Replace it" (design §4.10), shown when
 * Pi Dash on this computer signed in as a new machine and the user has an
 * offline one with the same hostname. Closing without choosing decides
 * nothing: the app asks again on its next launch.
 */
export function DesktopReplaceMachineDialog() {
  const [state, setState] = useState<{ account: string; prompt: ReplacePrompt } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ReplaceOutcome | null>(null);

  useEffect(() => {
    registerReplacePrompt((next) => {
      setState(next);
      setBusy(false);
      setError(null);
      setOutcome(null);
    });
    return () => registerReplacePrompt(null);
  }, []);

  const close = () => !busy && setState(null);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const confirm = () =>
    run(async () => {
      if (!state) return;
      setOutcome(await replaceMachine(state.prompt));
    });

  const decline = () =>
    run(async () => {
      if (!state) return;
      await declineReplace(state.account, state.prompt);
      setState(null);
    });

  const candidate = state?.prompt.candidate;
  const name = candidate ? candidate.label || candidate.display_name || candidate.host_label : "";

  return (
    <ModalCore isOpen={state !== null} handleClose={close} position={EModalPosition.CENTER} width={EModalWidth.XL}>
      <div data-prevent-outside-click className="flex flex-col gap-4 p-5">
        <h3 className="text-16 font-medium text-primary">
          Is this {name}, last seen {lastSeen(candidate?.last_seen_at ?? null)}?
        </h3>
        {outcome ? (
          <p className="text-13 text-secondary">
            {name} was replaced by this computer. {resultText(outcome)}
          </p>
        ) : (
          <p className="text-13 text-secondary">
            Pi Dash on this computer signed in as a new machine. If this is the same computer as {name}, replace it: its
            runners, pins and pods move here, and the built-in engine picks up its projects again. If it is a different
            computer, keep it.
          </p>
        )}
        {outcome?.failed.length ? (
          <p className="text-13 text-danger-primary">
            Could not turn the built-in engine back on for{" "}
            {outcome.failed.map((f) => `${f.project} (${f.error})`).join(", ")}. Turn it on from Built-in engine
            settings.
          </p>
        ) : null}
        {error && <p className="text-13 text-danger-primary">Replacing it did not finish: {error}</p>}
        <div className="flex justify-end gap-2">
          {outcome ? (
            <Button onClick={() => setState(null)}>Close</Button>
          ) : (
            <>
              <Button variant="secondary" onClick={() => void decline()} disabled={busy}>
                No, keep it
              </Button>
              <Button onClick={() => void confirm()} loading={busy} disabled={busy}>
                Replace it
              </Button>
            </>
          )}
        </div>
      </div>
    </ModalCore>
  );
}
