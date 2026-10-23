// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Managing the on-machine chat history (NEWFRONT-589, DESK-009). Two
// dialogs: one renames a conversation, one removes a conversation or the
// whole account's history. The history column owns the row menu and opens
// these; the page owns the writes. Nothing here decides what to keep after
// a removal — the page reads `selectionAfterDelete` for that.

import * as React from "react";
import { Button, Dialog, Input } from "@pidash/kit";

import {
  CLEAR_HISTORY_TITLE,
  DELETE_CHAT_BODY,
  DELETE_CHAT_TITLE,
  clearHistoryBody,
  historyTitleError,
} from "./historyActions.js";

export const RENAME_DIALOG_TITLE = "Rename conversation";
export const RENAME_FIELD_LABEL = "Name";
export const RENAME_CONFIRM_LABEL = "Save name";

export interface RenameChatDialogProps {
  /** The conversation being renamed. */
  target: { id: string; title: string };
  onCancel: () => void;
  onSubmit: (title: string) => void;
  /** A write is on its way; the dialog stays up until it lands. */
  busy?: boolean;
}

export function RenameChatDialog({
  target,
  onCancel,
  onSubmit,
  busy = false,
}: RenameChatDialogProps): React.ReactElement {
  const [value, setValue] = React.useState("");
  const [touched, setTouched] = React.useState(false);
  const fieldId = React.useId();

  // Each newly opened conversation starts from its current name, not from
  // whatever the previous rename left in the box.
  React.useEffect(() => {
    setValue(target.title);
    setTouched(false);
  }, [target.id, target.title]);

  const error = touched ? historyTitleError(value) : null;
  const submit = (): void => {
    setTouched(true);
    if (historyTitleError(value) !== null) return;
    onSubmit(value.trim());
  };

  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next && !busy) onCancel();
      }}
      title={RENAME_DIALOG_TITLE}
      nonDismissable={busy}
      bodyClassName="mt-(--space-4)"
      actions={
        <>
          <Button variant="ghost" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={submit}>
            {RENAME_CONFIRM_LABEL}
          </Button>
        </>
      }
    >
      <Input
        id={fieldId}
        label={RENAME_FIELD_LABEL}
        value={value}
        disabled={busy}
        {...(error === null ? {} : { error })}
        onChange={(event) => {
          setValue(event.target.value);
          setTouched(true);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") submit();
        }}
      />
    </Dialog>
  );
}

export interface HistoryRemovalDialogProps {
  /** `single` removes one conversation, `all` wipes the account's history. */
  choice: { scope: "single" } | { scope: "all" };
  /** How many conversations the account currently stores (for the copy). */
  sessionCount: number;
  onCancel: () => void;
  onConfirm: () => void;
  busy?: boolean;
}

export function HistoryRemovalDialog({
  choice,
  sessionCount,
  onCancel,
  onConfirm,
  busy = false,
}: HistoryRemovalDialogProps): React.ReactElement {
  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next && !busy) onCancel();
      }}
      title={choice.scope === "all" ? CLEAR_HISTORY_TITLE : DELETE_CHAT_TITLE}
      description={choice.scope === "all" ? clearHistoryBody(sessionCount) : DELETE_CHAT_BODY}
      nonDismissable={busy}
      bodyClassName="mt-(--space-2)"
      actions={
        <>
          <Button variant="ghost" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" loading={busy} onClick={onConfirm}>
            {choice.scope === "all" ? "Clear history" : "Delete chat"}
          </Button>
        </>
      }
    >
      {null}
    </Dialog>
  );
}
