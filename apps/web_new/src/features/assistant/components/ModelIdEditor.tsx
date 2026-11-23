// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-key model-ID editor (NEWPIDASH-394, design sections 7.2 and 7.3).
// One row per typed model ID with its verification status, the
// provider's message on failure, test progress while a probe runs, and a
// per-ID Test button; an add row grows the list. Removing the last ID
// asks for confirmation inline — confirming deletes the key, so the
// caller names it for the prompt. The editor is controlled: the caller
// owns the list and persists every add and remove (immediately on a
// connection row, on Save in the provider form). A "Save anyway" retry
// appears only while the caller offers it.
import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import {
  lastModelRemoveBody,
  lastModelRemoveTitle,
  MODEL_ID_ADD_ACTION,
  MODEL_ID_ADD_PLACEHOLDER,
  MODEL_ID_CANCEL_REMOVE_ACTION,
  MODEL_ID_CONFIRM_REMOVE_ACTION,
  MODEL_ID_REMOVE_ACTION,
  MODEL_ID_STATUS_FAILED,
  MODEL_ID_STATUS_UNVERIFIED,
  MODEL_ID_STATUS_VERIFIED,
  MODEL_ID_TEST_ACTION,
  MODEL_ID_TESTING_LABEL,
  MODEL_IDS_HEADING,
  MODEL_IDS_SAVE_ANYWAY_ACTION,
} from "../lib/copy.js";
import { modelIdStatus } from "../lib/byokModels.js";

export interface ModelIdEditorProps {
  /** The key's model IDs, in stored (or draft) order. */
  ids: readonly string[];
  /** Passing stamps by model ID (`model_verified_at`); absent entries are untested. */
  verifiedAt: Readonly<Record<string, string | null | undefined>>;
  /** The provider message by model ID from the last refused write or test. */
  failures: Readonly<Record<string, string>>;
  /** IDs with a probe or save in flight; their rows show progress. */
  pendingIds: readonly string[];
  /** Disables every control while a save runs. */
  disabled: boolean;
  /** Shows the per-ID Test buttons; false where nothing is stored to test yet (the add form). */
  testable: boolean;
  /**
   * Whether removing the last ID asks for confirmation: true where the
   * last ID owns a stored key (the confirm deletes both), false in the
   * add form where the list is still a draft.
   */
  confirmLastRemove: boolean;
  /** The key label naming the key the last-ID prompt deletes ("BYOK:host", a provider name). */
  keyLabel: string;
  onAdd(model: string): void;
  onRemove(model: string): void;
  onTest(model: string): void;
  /** Offered while the last refused write left only unverified IDs; null hides the retry. */
  onSaveAnyway: (() => void) | null;
}

export function ModelIdEditor(props: ModelIdEditorProps): React.ReactElement {
  const { ids, verifiedAt, failures, pendingIds, disabled, testable, keyLabel } = props;
  const [draft, setDraft] = React.useState("");
  const [confirming, setConfirming] = React.useState<string | null>(null);
  const pending = React.useMemo(() => new Set(pendingIds), [pendingIds]);

  // A confirmed removal shrinks the list; drop a stale prompt with it.
  React.useEffect(() => {
    if (confirming !== null && !ids.includes(confirming)) setConfirming(null);
  }, [ids, confirming]);

  const add = (): void => {
    const model = draft.trim();
    if (model === "" || ids.includes(model)) return;
    setDraft("");
    props.onAdd(model);
  };

  const remove = (model: string): void => {
    // The last ID owns a stored key: confirm, then the caller deletes
    // both. In the add form the list is still a draft, so it just goes.
    if (ids.length === 1 && props.confirmLastRemove) {
      setConfirming(model);
      return;
    }
    props.onRemove(model);
  };

  return (
    <div data-testid="byok-model-editor" className="flex flex-col gap-(--space-2)">
      <h3 className="text-body font-medium text-(--text)">{MODEL_IDS_HEADING}</h3>
      {ids.length === 0 ? null : (
        <ul className="flex flex-col gap-(--space-2)">
          {ids.map((model) => {
            const status = modelIdStatus(model, verifiedAt, failures);
            const detail = failures[model] ?? null;
            const testing = pending.has(model);
            const rowBusy = disabled || testing;
            return (
              <li
                key={model}
                data-testid="byok-model-row"
                className="flex flex-wrap items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)"
              >
                <span className="text-body min-w-0 flex-1 break-all text-(--text)">{model}</span>
                {testing ? (
                  <Spinner label={MODEL_ID_TESTING_LABEL} size="small" />
                ) : (
                  <span data-testid="byok-model-status" className="text-caption text-(--text-muted)">
                    {status === "verified"
                      ? MODEL_ID_STATUS_VERIFIED
                      : status === "failed"
                        ? MODEL_ID_STATUS_FAILED
                        : MODEL_ID_STATUS_UNVERIFIED}
                  </span>
                )}
                {detail === null || detail === "" ? null : (
                  <span data-testid="byok-model-detail" className="text-caption w-full text-(--danger)">
                    {detail}
                  </span>
                )}
                {testable ? (
                  <Button
                    size="small"
                    variant="secondary"
                    disabled={rowBusy}
                    aria-label={`${MODEL_ID_TEST_ACTION} ${model}`}
                    onClick={() => props.onTest(model)}
                  >
                    {MODEL_ID_TEST_ACTION}
                  </Button>
                ) : null}
                <Button
                  size="small"
                  variant="danger"
                  disabled={disabled}
                  aria-label={`${MODEL_ID_REMOVE_ACTION} ${model}`}
                  onClick={() => remove(model)}
                >
                  {MODEL_ID_REMOVE_ACTION}
                </Button>
              </li>
            );
          })}
        </ul>
      )}

      <div className="flex flex-wrap items-center gap-(--space-2)">
        <input
          aria-label={MODEL_ID_ADD_PLACEHOLDER}
          placeholder={MODEL_ID_ADD_PLACEHOLDER}
          value={draft}
          disabled={disabled}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            add();
          }}
          className="text-body h-(--control-height) min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
        />
        <Button size="small" variant="secondary" disabled={disabled || draft.trim() === ""} onClick={add}>
          {MODEL_ID_ADD_ACTION}
        </Button>
        {props.onSaveAnyway === null ? null : (
          <Button size="small" variant="primary" disabled={disabled} onClick={props.onSaveAnyway}>
            {MODEL_IDS_SAVE_ANYWAY_ACTION}
          </Button>
        )}
      </div>

      {confirming === null ? null : (
        <div
          role="group"
          aria-label={lastModelRemoveTitle(confirming)}
          className="flex flex-wrap items-center justify-between gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-2)"
        >
          <span className="text-body text-(--text)">{lastModelRemoveBody(confirming, keyLabel)}</span>
          <span className="flex items-center gap-(--space-2)">
            <Button
              size="small"
              variant="danger"
              onClick={() => {
                setConfirming(null);
                props.onRemove(confirming);
              }}
            >
              {MODEL_ID_CONFIRM_REMOVE_ACTION}
            </Button>
            <Button size="small" variant="ghost" onClick={() => setConfirming(null)}>
              {MODEL_ID_CANCEL_REMOVE_ACTION}
            </Button>
          </span>
        </div>
      )}
    </div>
  );
}
