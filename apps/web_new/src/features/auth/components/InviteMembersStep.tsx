// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Invite-members onboarding step (AUTH-037). Blank address-plus-role rows
// that grow on demand; sending drops the empties, posts the filled rows
// against the first workspace, and finishes onboarding, while deferring
// finishes without sending anything. Enter never submits. Written from
// parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

import { ROLE_MEMBER, useWorkspaces } from "../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useSendWorkspaceInvites } from "../api.js";
import { INVITE_INITIAL_ROW_COUNT, WORKSPACE_INVITE_ROLES } from "../workspace.js";
import { isValidEmail } from "./SignInCard.js";

export const INVITE_SUCCESS_TITLE = "Invitations sent";
export const INVITE_FAILED_TITLE = "Could not send invitations";
export const INVITE_ROW_ERROR_MESSAGE = "Enter a valid email address.";

interface InviteRow {
  key: number;
  email: string;
  role: number;
}

function blankRow(key: number): InviteRow {
  return { key, email: "", role: ROLE_MEMBER };
}

export interface InviteMembersStepProps {
  onAdvance: () => void;
}

export function InviteMembersStep({ onAdvance }: InviteMembersStepProps): React.ReactElement {
  const workspaces = useWorkspaces();
  const send = useSendWorkspaceInvites();

  const [rows, setRows] = React.useState<InviteRow[]>(() =>
    Array.from({ length: INVITE_INITIAL_ROW_COUNT }, (_unused, index) => blankRow(index))
  );
  const nextKey = React.useRef(INVITE_INITIAL_ROW_COUNT);

  const target = workspaces.data?.[0];
  const sending = send.isPending;
  const rowFilled = (row: InviteRow): boolean => row.email.trim() !== "";
  const rowValid = (row: InviteRow): boolean => !rowFilled(row) || isValidEmail(row.email);
  const canSend =
    target !== undefined && !sending && rows.some((row) => rowFilled(row) && rowValid(row)) && rows.every(rowValid);

  const setRow = (key: number, patch: Partial<InviteRow>): void => {
    setRows((current) => current.map((row) => (row.key === key ? { ...row, ...patch } : row)));
  };

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    if (target === undefined || !canSend) return;
    const emails = rows.filter((row) => rowFilled(row)).map((row) => ({ email: row.email.trim(), role: row.role }));
    try {
      await send.mutateAsync({ slug: target.slug, emails });
      notifySuccess(INVITE_SUCCESS_TITLE);
      onAdvance();
    } catch (error) {
      notifyError(INVITE_FAILED_TITLE, (error as Error).message);
    }
  };

  return (
    <section aria-labelledby="invite-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="invite-heading" className="text-h2 text-(--text)">
          Invite your team
        </h1>
        <p className="text-body text-(--text-muted)">Invite teammates to join your workspace.</p>
      </div>

      <form
        onSubmit={(event) => void submit(event)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.preventDefault();
        }}
        className="flex flex-col gap-(--space-4)"
      >
        <div aria-hidden="true" className="grid grid-cols-10 gap-(--space-4)">
          <span className="text-caption col-span-6 font-medium text-(--text-muted)">Email</span>
          <span className="text-caption col-span-4 font-medium text-(--text-muted)">Role</span>
        </div>

        <div className="flex flex-col gap-(--space-3)">
          {rows.map((row, index) => (
            <div key={row.key} className="flex flex-col gap-(--space-1)">
              <div className="grid grid-cols-10 items-center gap-(--space-4)">
                <input
                  type="text"
                  aria-label={`Email address ${index + 1}`}
                  aria-invalid={!rowValid(row)}
                  value={row.email}
                  autoFocus={index === 0}
                  autoComplete="off"
                  placeholder="colleague@example.com"
                  onChange={(event) => setRow(row.key, { email: event.target.value })}
                  className="text-body col-span-6 rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) text-(--text)"
                />
                <select
                  aria-label={`Role ${index + 1}`}
                  value={row.role}
                  onChange={(event) => setRow(row.key, { role: Number(event.target.value) })}
                  className="text-body col-span-3 rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) text-(--text)"
                >
                  {WORKSPACE_INVITE_ROLES.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
                {rows.length > 1 ? (
                  <button
                    type="button"
                    aria-label={`Remove invite ${index + 1}`}
                    onClick={() => setRows((current) => current.filter((entry) => entry.key !== row.key))}
                    className="text-body cursor-pointer justify-self-end text-(--text-muted)"
                  >
                    Remove
                  </button>
                ) : null}
              </div>
              {!rowValid(row) ? <p className="text-body text-(--danger)">{INVITE_ROW_ERROR_MESSAGE}</p> : null}
            </div>
          ))}
        </div>

        <button
          type="button"
          onClick={() => {
            const key = nextKey.current;
            nextKey.current += 1;
            setRows((current) => [...current, blankRow(key)]);
          }}
          className="text-body cursor-pointer self-start font-medium text-(--accent)"
        >
          Add another
        </button>

        <div className="flex flex-col gap-(--space-2)">
          <Button type="submit" variant="primary" size="large" loading={sending} disabled={!canSend}>
            Continue
          </Button>
          <Button type="button" variant="ghost" size="large" onClick={onAdvance} disabled={sending}>
            Invite later
          </Button>
        </div>
      </form>
    </section>
  );
}
