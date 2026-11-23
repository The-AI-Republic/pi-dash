// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved connections (parity/specs/cloud.md §7, CLOUD-032–034, NEWPIDASH-394).
// One card per connection: its title and markers, a kind-appropriate model
// control, and the row's actions test, remove (self-supplied) or disconnect
// (provider account). Which model answers is chosen per surface in the
// Models section above, never per row. Removing asks inline first; the
// managed rows are permanent. A hosted row a stale session cannot reach
// keeps its place but says how to fix the session (CLOUD-034).

import type { ModelTestResult } from "@pidash/api-client";
import { modelTestResultsFrom } from "@pidash/api-client";
import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { ModelIdEditor, modelFailedIds, modelFailureDetails, saveAnywayModels } from "../../assistant/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { actionFailureDetail } from "../api/mutations.js";
import {
  CANCEL_ACTION,
  CONFIRM_REMOVE_ACTION,
  DISCONNECT_ACTION,
  MANAGE_USAGE_HREF,
  MANAGE_USAGE_LABEL,
  MODEL_FAILED_TITLE,
  NEEDS_CREDIT_NOTICE,
  REMOVE_ACTION,
  REMOVE_CONFIRM_BODY,
  REMOVE_CONFIRM_TITLE,
  SIGN_IN_ACTION,
  SIGN_IN_PENDING_ACTION,
  SIGN_IN_UNAVAILABLE_BODY,
  SIGN_IN_BODY,
  STALE_NOTICE,
  TEST_ACTION,
  TEST_FAILED_FALLBACK,
  TEST_FAILED_TITLE,
  TOP_UP_LABEL,
} from "../lib/copy.js";
import type { ConnectionRow } from "../lib/rows.js";

export interface ConnectionsListProps {
  rows: readonly ConnectionRow[];
  /** Provider sign-in is offered by this deployment. */
  signInAvailable: boolean;
  signInPending: boolean;
  testing: boolean;
  busyId: string | null;
  /** Wallet address for the "Needs credit" top-up link; null hides it. */
  topUpHref: string | null;
  onCommitModel(row: ConnectionRow, model: string): void;
  /**
   * Replace a key's model-ID list (NEWPIDASH-394, design section 6.3).
   * Rejects on a refused write; the card reads the per-ID results off
   * the error and shows them inline.
   */
  onSaveModels(row: ConnectionRow, models: string[], saveUnverified: boolean): Promise<void>;
  /** Smoke-test one stored model ID (design section 6.4). */
  onTestModel(row: ConnectionRow, model: string): Promise<ModelTestResult>;
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
  // Per-ID write/test state for the key's model editor (NEWPIDASH-394,
  // design sections 7.2 and 7.3): the provider message by model from
  // the last refused write or test, the IDs with a probe in flight,
  // and whether a "Save anyway" retry is on offer. `attempted` is the
  // last list a write tried: a refused write saves nothing, so the
  // editor renders the attempted IDs too, giving each failure a row.
  const [failures, setFailures] = React.useState<Record<string, string>>({});
  /** Refused/tested IDs the provider rejected (`status: "failed"`); the rest of `failures` is unverified. */
  const [failedIds, setFailedIds] = React.useState<readonly string[]>([]);
  const [pendingIds, setPendingIds] = React.useState<readonly string[]>([]);
  const [saveAnyway, setSaveAnyway] = React.useState(false);
  const [attempted, setAttempted] = React.useState<readonly string[] | null>(null);

  // Persist a new model-ID list for the key. A refused write (`400
  // model_test_failed`, nothing saved) lands inline per ID; any other
  // failure notifies. A confirmed last-ID removal deletes the key, so
  // it never reaches this write.
  const persist = async (models: string[], saveUnverified: boolean): Promise<void> => {
    const fresh = models.filter((model) => !row.models.includes(model));
    setAttempted(models);
    setPendingIds(fresh);
    try {
      await props.onSaveModels(row, models, saveUnverified);
      setAttempted(null);
      setFailures({});
      setFailedIds([]);
      setSaveAnyway(false);
    } catch (error) {
      const results = modelTestResultsFrom(error);
      if (results === null) {
        setAttempted(null);
        notifyError(MODEL_FAILED_TITLE, actionFailureDetail(error, MODEL_FAILED_TITLE));
      } else {
        setFailures(modelFailureDetails(results));
        setFailedIds(modelFailedIds(results));
        setSaveAnyway(saveAnywayModels(results) !== null);
      }
    } finally {
      setPendingIds([]);
    }
  };

  // The editor's list: the stored IDs plus any attempted-but-unsaved
  // extras, so a refused write's failures each keep their row.
  const displayed = React.useMemo(() => {
    if (attempted === null) return row.models;
    const extra = attempted.filter((model) => !row.models.includes(model));
    return [...row.models, ...extra];
  }, [row.models, attempted]);

  const testOne = async (model: string): Promise<void> => {
    setPendingIds([model]);
    try {
      const result = await props.onTestModel(row, model);
      if (result.status === "verified") {
        setFailures((current) => {
          if (current[model] === undefined) return current;
          const next = { ...current };
          delete next[model];
          return next;
        });
        setFailedIds((current) => current.filter((kept) => kept !== model));
      } else if (result.status === "failed") {
        setFailures((current) => ({ ...current, [model]: result.detail ?? result.code ?? "" }));
        setFailedIds((current) => (current.includes(model) ? current : [...current, model]));
      } else {
        setFailures((current) => ({ ...current, [model]: result.detail ?? result.code ?? "" }));
        setFailedIds((current) => current.filter((kept) => kept !== model));
      }
    } catch (error) {
      notifyError(TEST_FAILED_TITLE, actionFailureDetail(error, TEST_FAILED_FALLBACK));
    } finally {
      setPendingIds([]);
    }
  };

  return (
    <li>
      <div data-testid={`assistant-connection-${row.id}`} className={CARD}>
        <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
          <div className="flex flex-wrap items-center gap-(--space-2)">
            <span data-testid="assistant-connection-title" className="text-body font-medium text-(--text)">
              {row.title}
            </span>
            {row.markers.map((marker) => (
              <span
                key={marker}
                className="text-caption rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
              >
                {marker}
              </span>
            ))}
          </div>
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
            {row.selected && row.modelControl === "select" ? (
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
          <ModelIdEditor
            ids={displayed}
            verifiedAt={row.modelVerifiedAt}
            failures={failures}
            failedIds={failedIds}
            pendingIds={pendingIds}
            disabled={busy}
            testable
            confirmLastRemove
            keyLabel={row.title}
            onAdd={(model) => void persist([...displayed, model], false)}
            onRemove={(model) => {
              const next = displayed.filter((kept) => kept !== model);
              if (next.length === 0) {
                props.onRemove(row);
              } else {
                void persist(next, false);
              }
            }}
            onTest={(model) => void testOne(model)}
            onSaveAnyway={saveAnyway ? () => void persist(displayed, true) : null}
          />
        ) : null}

        {row.modelCredit === "blocked" ? (
          <p role="status" className="text-body text-(--text-muted)">
            {NEEDS_CREDIT_NOTICE}{" "}
            {props.topUpHref !== null ? (
              <a href={props.topUpHref} target="_blank" rel="noopener noreferrer" className="text-body text-(--accent)">
                {TOP_UP_LABEL}
              </a>
            ) : null}
          </p>
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
          <option key={choice.value} value={choice.value} disabled={!choice.selectable}>
            {choice.badge === null ? choice.label : `${choice.label} (${choice.badge})`}
          </option>
        ))}
      </select>
    </label>
  );
}
