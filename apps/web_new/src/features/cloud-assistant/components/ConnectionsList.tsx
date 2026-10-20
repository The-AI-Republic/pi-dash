// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved connections (parity/specs/cloud.md §7, CLOUD-032–034). One card per
// connection: a radio chooses the active row, a kind-appropriate model
// control edits it, and the row's actions test, remove (self-supplied) or
// disconnect (provider account). Removing asks inline first; the managed
// rows are permanent. A hosted row a stale session cannot reach keeps its
// place but says how to fix the session (CLOUD-034).

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import {
  CANCEL_ACTION,
  CONFIRM_REMOVE_ACTION,
  DISCONNECT_ACTION,
  MANAGE_USAGE_HREF,
  MANAGE_USAGE_LABEL,
  REMOVE_ACTION,
  REMOVE_CONFIRM_BODY,
  REMOVE_CONFIRM_TITLE,
  SIGN_IN_ACTION,
  SIGN_IN_PENDING_ACTION,
  SIGN_IN_UNAVAILABLE_BODY,
  SIGN_IN_BODY,
  STALE_NOTICE,
  TEST_ACTION,
} from "../lib/copy.js";
import type { ConnectionRow } from "../lib/rows.js";

export const CONNECTION_RADIO_NAME = "assistant-active-connection";

export interface ConnectionsListProps {
  rows: readonly ConnectionRow[];
  /** Provider sign-in is offered by this deployment. */
  signInAvailable: boolean;
  signInPending: boolean;
  testing: boolean;
  busyId: string | null;
  onSelect(row: ConnectionRow): void;
  onCommitModel(row: ConnectionRow, model: string): void;
  onTest(): void;
  onRemove(row: ConnectionRow): void;
  onDisconnect(row: ConnectionRow): void;
  onSignIn(): void;
}

export function ConnectionsList(props: ConnectionsListProps): React.ReactElement {
  const { rows } = props;
  return (
    <ul aria-label="Saved connections" className="flex flex-col gap-(--space-3)">
      {rows.map((row) => (
        <ConnectionCard key={row.id} row={row} {...props} />
      ))}
    </ul>
  );
}

const CARD =
  "flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--surface) p-(--space-4)";

function ConnectionCard(props: ConnectionsListProps & { row: ConnectionRow }): React.ReactElement {
  const { row, busyId, testing, signInAvailable, signInPending } = props;
  const [confirming, setConfirming] = React.useState(false);
  const busy = busyId === row.id;

  return (
    <li>
      <div data-testid={`assistant-connection-${row.id}`} className={CARD}>
        <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
          <label className="flex cursor-pointer flex-wrap items-center gap-(--space-2)">
            <input
              type="radio"
              name={CONNECTION_RADIO_NAME}
              checked={row.selected}
              disabled={!row.selectable || busy}
              aria-label={row.title}
              onChange={() => props.onSelect(row)}
            />
            <span className="text-body font-medium text-(--text)">{row.title}</span>
            {row.markers.map((marker) => (
              <span
                key={marker}
                className="text-caption rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
              >
                {marker}
              </span>
            ))}
          </label>
          <div className="flex flex-wrap items-center gap-(--space-2)">
            {row.usesChatgptPlan ? (
              <a
                href={MANAGE_USAGE_HREF}
                target="_blank"
                rel="noopener noreferrer"
                className="text-body text-(--accent)"
              >
                {MANAGE_USAGE_LABEL}
              </a>
            ) : null}
            {row.selected && row.modelControl !== "hidden" ? (
              <Button size="small" variant="secondary" loading={testing} onClick={props.onTest}>
                {TEST_ACTION}
              </Button>
            ) : null}
            {row.removable ? (
              <Button size="small" variant="danger" disabled={busy} onClick={() => setConfirming(true)}>
                {REMOVE_ACTION}
              </Button>
            ) : null}
            {row.disconnectable ? (
              <Button size="small" variant="danger" disabled={busy} onClick={() => props.onDisconnect(row)}>
                {DISCONNECT_ACTION}
              </Button>
            ) : null}
            {row.disconnected && signInAvailable ? (
              <Button size="small" variant="primary" loading={signInPending} onClick={props.onSignIn}>
                {signInPending ? SIGN_IN_PENDING_ACTION : SIGN_IN_ACTION}
              </Button>
            ) : null}
          </div>
        </div>

        {row.disconnected ? (
          <p className="text-body text-(--text-muted)">{signInAvailable ? SIGN_IN_BODY : SIGN_IN_UNAVAILABLE_BODY}</p>
        ) : null}

        {row.stale ? (
          <p role="status" className="text-body text-(--text-muted)">
            {STALE_NOTICE}
          </p>
        ) : null}

        {row.modelControl === "select" ? (
          <ModelPicker row={row} disabled={busy} onCommit={props.onCommitModel} />
        ) : null}
        {row.modelControl === "input" ? (
          <FreeModelField row={row} disabled={busy} onCommit={props.onCommitModel} />
        ) : null}

        {row.verifiedAt !== null ? (
          <p className="text-caption text-(--text-muted)">
            Verified <time dateTime={row.verifiedAt}>{row.verifiedAt}</time>
          </p>
        ) : null}

        {confirming ? (
          <div
            role="group"
            aria-label={REMOVE_CONFIRM_TITLE}
            className="flex flex-wrap items-center justify-between gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-2)"
          >
            <span className="text-body text-(--text)">
              {REMOVE_CONFIRM_TITLE} <span className="text-(--text-muted)">{REMOVE_CONFIRM_BODY}</span>
            </span>
            <span className="flex items-center gap-(--space-2)">
              <Button
                size="small"
                variant="danger"
                onClick={() => {
                  setConfirming(false);
                  props.onRemove(row);
                }}
              >
                {CONFIRM_REMOVE_ACTION}
              </Button>
              <Button size="small" variant="ghost" onClick={() => setConfirming(false)}>
                {CANCEL_ACTION}
              </Button>
            </span>
          </div>
        ) : null}
        {busy ? <Spinner label="Saving connection" size="small" /> : null}
      </div>
    </li>
  );
}

function ModelPicker(props: {
  row: ConnectionRow;
  disabled: boolean;
  onCommit(row: ConnectionRow, model: string): void;
}): React.ReactElement {
  const { row } = props;
  return (
    <label className="text-body flex flex-wrap items-center gap-(--space-2) text-(--text-muted)">
      Model
      <select
        aria-label={`Model for ${row.title}`}
        value={row.modelName}
        disabled={props.disabled}
        onChange={(event) => props.onCommit(row, event.target.value)}
        className="text-body h-(--control-height) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
      >
        {row.modelChoices.map((choice) => (
          <option key={choice.value} value={choice.value}>
            {choice.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function FreeModelField(props: {
  row: ConnectionRow;
  disabled: boolean;
  onCommit(row: ConnectionRow, model: string): void;
}): React.ReactElement {
  const { row } = props;
  const [value, setValue] = React.useState(row.modelName);
  const listId = "assistant-known-byok-models";
  return (
    <label className="text-body flex flex-wrap items-center gap-(--space-2) text-(--text-muted)">
      Model
      <input
        aria-label={`Model for ${row.title}`}
        value={value}
        list={listId}
        disabled={props.disabled}
        onChange={(event) => setValue(event.target.value)}
        onBlur={() => {
          const next = value.trim();
          if (next !== "" && next !== row.modelName) props.onCommit(row, next);
        }}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return;
          event.preventDefault();
          const next = value.trim();
          if (next !== "" && next !== row.modelName) props.onCommit(row, next);
        }}
        className="text-body h-(--control-height) min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
      />
      <datalist id={listId}>
        {row.modelSuggestions.map((model) => (
          <option key={model} value={model} />
        ))}
      </datalist>
    </label>
  );
}
