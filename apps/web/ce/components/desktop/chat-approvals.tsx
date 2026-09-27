/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Community-edition wording and controls for the built-in engine's chat
 * approvals (PDASHOSS01-173). Kept in the `components/desktop` seam so an
 * edition can replace the copy without touching `RunnerChatPage`. Two pieces:
 *
 * * {@link ChatApprovalModeSelect} — the mode control (ask / workspace / full
 *   access) shown while chatting.
 * * {@link ChatApprovalPrompt} — the inline prompt rendered in the conversation
 *   when the engine asks to run a command, edit a file, or reach the network.
 *
 * Both are presentational; the page owns the transport calls and the pending
 * state derived from the chat event stream.
 */

import { Button } from "@pi-dash/ui";
import type { TApprovalDecision, TApprovalKind, TApprovalMode } from "@pi-dash/types";

/** The mode spectrum, in order from most to least cautious. */
export const APPROVAL_MODES: readonly TApprovalMode[] = ["ask", "workspace", "full_access"];

interface ModeCopy {
  label: string;
  help: string;
}

/** Short label + one-line explanation for each mode. Edition-overridable. */
export const APPROVAL_MODE_COPY: Record<TApprovalMode, ModeCopy> = {
  ask: {
    label: "Ask every time",
    help: "Approve each command, edit, or network call before it runs.",
  },
  workspace: {
    label: "Workspace",
    help: "Run edits inside the working copy; ask before anything outside it.",
  },
  full_access: {
    label: "Full access",
    help: "Run everything without asking. Denylisted commands are still refused.",
  },
};

/** Human wording for what the engine is asking to do. */
const APPROVAL_KIND_LABELS: Record<TApprovalKind, string> = {
  command_execution: "wants to run a command",
  file_change: "wants to edit a file",
  network_access: "wants to make a network call",
  other: "is requesting approval",
};

/** One pending approval, projected from a `chat_approval_request` event. */
export interface PendingChatApproval {
  localApprovalId: string;
  kind: TApprovalKind;
  reason: string;
  /** Free-form payload spread from the frame (command/cwd/path/url/…). */
  payload: Record<string, unknown>;
  /** ISO deadline after which the runner expires the request (its TTL). */
  expiresAt: string | null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** The most specific thing to show the user for a pending request. */
export function approvalTarget(request: PendingChatApproval): string | null {
  const p = request.payload;
  return (
    asString(p.command) ?? asString(p.path) ?? asString(p.url) ?? asString(p.destination) ?? asString(p.host) ?? null
  );
}

/**
 * The mode control. A small segmented button group; the active mode is filled.
 * `onChange` writes through to the transport; the runner applies it to the next
 * thread, never a turn already in flight.
 */
export function ChatApprovalModeSelect(props: {
  value: TApprovalMode;
  onChange: (mode: TApprovalMode) => void;
  disabled?: boolean;
}) {
  const { value, onChange, disabled } = props;
  return (
    <div className="flex items-center gap-1" role="group" aria-label="Approval mode">
      <span className="mr-1 text-11 text-tertiary">Approvals</span>
      {APPROVAL_MODES.map((mode) => {
        const copy = APPROVAL_MODE_COPY[mode];
        const active = mode === value;
        return (
          <button
            key={mode}
            type="button"
            disabled={disabled}
            aria-pressed={active}
            title={copy.help}
            onClick={() => onChange(mode)}
            className={`rounded px-2 py-1 text-11 transition-colors disabled:opacity-50 ${
              active ? "bg-surface-3 font-medium text-primary" : "text-secondary hover:bg-surface-2"
            }`}
          >
            {copy.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * The inline prompt shown in the conversation while the turn is parked waiting
 * for a decision. Approve / Deny answer the request; the turn continues either
 * way. "Always allow this session" maps to the engine's `accept_for_session`.
 */
export function ChatApprovalPrompt(props: {
  request: PendingChatApproval;
  onDecide: (decision: TApprovalDecision) => void;
  busy?: boolean;
}) {
  const { request, onDecide, busy } = props;
  const target = approvalTarget(request);
  return (
    <div
      role="alertdialog"
      aria-label="Approval requested"
      className="rounded-md border border-warning-subtle bg-warning-subtle px-3 py-3 text-12"
    >
      <div className="text-warning-secondary">
        The agent {APPROVAL_KIND_LABELS[request.kind] ?? APPROVAL_KIND_LABELS.other}:
      </div>
      {target && (
        <pre className="font-mono mt-1 overflow-x-auto rounded bg-surface-1 px-2 py-1 text-11 text-primary">
          {target}
        </pre>
      )}
      {request.reason && <div className="mt-1 text-11 text-tertiary">{request.reason}</div>}
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <Button variant="primary" size="sm" disabled={busy} onClick={() => onDecide("accept")}>
          Approve
        </Button>
        <Button variant="neutral-primary" size="sm" disabled={busy} onClick={() => onDecide("decline")}>
          Deny
        </Button>
        {request.kind === "command_execution" && (
          <Button
            variant="neutral-primary"
            size="sm"
            disabled={busy}
            title="Approve and stop asking for this command for the rest of the session"
            onClick={() => onDecide("accept_for_session")}
          >
            Always allow this session
          </Button>
        )}
      </div>
    </div>
  );
}
