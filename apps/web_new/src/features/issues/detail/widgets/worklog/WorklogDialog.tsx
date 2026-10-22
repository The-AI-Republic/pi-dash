// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Log-time dialog (parity/specs/issues.md §16, ISS-206, cloud leg). The
// minutes worked, the day worked, and an optional note. The dialog
// mounts per open, so its fields initialize from the edited entry (or
// from blank defaults) exactly once; a save with invalid minutes or no
// day explains itself inline instead of submitting.

import { Button, Dialog, Input } from "@pidash/kit";
import type { WorklogEntryCreate } from "@pidash/api-client";
import * as React from "react";

import { WORKLOG_DIALOG_TITLE_EDIT, WORKLOG_DIALOG_TITLE_LOG } from "./format.js";

export interface WorklogDialogInitial {
  minutes: number;
  logged_on: string;
  note: string;
}

/** Today's date on the viewer's calendar, as YYYY-MM-DD. */
export function todayLocalISO(now: Date = new Date()): string {
  const month = `${now.getMonth() + 1}`.padStart(2, "0");
  const day = `${now.getDate()}`.padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/** Whether the typed minutes read as a positive whole number. */
export function parseMinutesInput(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const minutes = Number(trimmed);
  return minutes > 0 ? minutes : null;
}

export interface WorklogDialogProps {
  /** Present when editing; absent when logging fresh time. */
  initial?: WorklogDialogInitial | undefined;
  /** In-flight save; the save control spins while true. */
  saving: boolean;
  onClose: () => void;
  onSubmit: (input: WorklogEntryCreate) => void;
}

export function WorklogDialog({ initial, saving, onClose, onSubmit }: WorklogDialogProps): React.ReactElement {
  const [minutesText, setMinutesText] = React.useState(initial === undefined ? "" : `${initial.minutes}`);
  const [dateText, setDateText] = React.useState(initial?.logged_on ?? todayLocalISO());
  const [noteText, setNoteText] = React.useState(initial?.note ?? "");
  const [attempted, setAttempted] = React.useState(false);
  const minutesRef = React.useRef<HTMLInputElement | null>(null);

  const minutes = parseMinutesInput(minutesText);

  return (
    <Dialog
      title={initial === undefined ? WORKLOG_DIALOG_TITLE_LOG : WORKLOG_DIALOG_TITLE_EDIT}
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      initialFocus={minutesRef}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={saving}
            onClick={() => {
              setAttempted(true);
              if (minutes === null || dateText === "") return;
              onSubmit({ minutes, logged_on: dateText, note: noteText.trim() });
            }}
          >
            {initial === undefined ? "Log time" : "Save"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="Minutes"
          type="number"
          min={1}
          step={1}
          inputMode="numeric"
          value={minutesText}
          inputRef={minutesRef}
          {...(attempted && minutes === null ? { error: "Enter whole minutes above zero." } : {})}
          onChange={(event) => {
            setMinutesText(event.target.value);
          }}
        />
        <Input
          label="Date"
          type="date"
          value={dateText}
          {...(attempted && dateText === "" ? { error: "Pick the day the work happened." } : {})}
          onChange={(event) => {
            setDateText(event.target.value);
          }}
        />
        <Input
          label="Note"
          hint="Optional"
          value={noteText}
          onChange={(event) => {
            setNoteText(event.target.value);
          }}
        />
      </div>
    </Dialog>
  );
}
