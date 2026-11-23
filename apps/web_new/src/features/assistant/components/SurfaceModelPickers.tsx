// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface model pickers for Assistant Settings (NEWPIDASH-394,
// design section 7.4). One picker per surface, each showing its
// effective choice and the fallback notice when the saved choice is
// set but unused. Choosing a model writes that surface's saved choice
// through PUT model-selection/{surface} and refreshes the selection so
// the composer agrees. The caller builds the choices: the cloud
// edition passes the merged catalog-plus-BYOK list (agent catalog
// only), the OSS edition its single key's list. The credit-blocked
// guard runs only when quota flags are passed (cloud).

import { Spinner } from "@pidash/kit";
import * as React from "react";

import type { AssistantSurface, ModelSelectionSurfaceState } from "@pidash/api-client";

import { isBlockedCommit, type ConnectionModelFlags } from "../../cloud-assistant/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { selectionFailureDetail, useModelSelection, useModelSelectionActions } from "../api/selection.js";
import {
  AGENT_LABEL,
  DESKTOP_ASSISTANT_LABEL,
  MODEL_PICKER_BLOCKED_BODY,
  MODEL_PICKER_BLOCKED_TITLE,
  MODEL_PICKER_FAILED_TITLE,
  SURFACE_MODEL_EMPTY_OPTION,
  WEB_ASSISTANT_LABEL,
} from "../lib/copy.js";
import {
  effectiveChoiceFor,
  fallbackNotice,
  mergedChoiceKey,
  pickerOptionLabel,
  OPENHUB_DEFAULT_MODEL,
} from "../lib/openhubPicker.js";
import type { MergedPickerChoice } from "../lib/openhubPicker.js";

/** One surface's picker: its label and its prebuilt choices. */
export interface SurfacePickerSpec {
  surface: AssistantSurface;
  label: string;
  choices: readonly MergedPickerChoice[];
}

export interface SurfaceModelPickersProps {
  /** Workspace scoping the selection read the pickers refresh. */
  workspaceSlug: string;
  /** The surfaces to offer, in display order. */
  surfaces: readonly SurfacePickerSpec[];
  /** Quota flags for the credit-blocked guard; null in OSS (no quota). */
  flags: ConnectionModelFlags | null;
  /** Label shown while the selection loads. */
  loadingLabel?: string | undefined;
}

export function SurfaceModelPickers({
  workspaceSlug,
  surfaces,
  flags,
  loadingLabel = "Loading models",
}: SurfaceModelPickersProps): React.ReactElement {
  const selection = useModelSelection(workspaceSlug);

  if (selection.isPending) {
    return <Spinner label={loadingLabel} size="small" />;
  }
  if (selection.isError || selection.data === undefined) {
    return (
      <p role="alert" className="text-body text-(--danger)">
        The model choices could not be loaded.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-(--space-4)">
      {surfaces.map((spec) => (
        <SurfacePickerRow
          key={spec.surface}
          workspaceSlug={workspaceSlug}
          spec={spec}
          flags={flags}
          saved={selection.data[spec.surface].saved}
          effective={selection.data[spec.surface].effective}
          fallbackReason={selection.data[spec.surface].fallback_reason}
        />
      ))}
    </div>
  );
}

interface SurfacePickerRowProps {
  workspaceSlug: string;
  spec: SurfacePickerSpec;
  flags: ConnectionModelFlags | null;
  saved: ModelSelectionSurfaceState["saved"];
  effective: ModelSelectionSurfaceState["effective"];
  fallbackReason: ModelSelectionSurfaceState["fallback_reason"];
}

function SurfacePickerRow({
  workspaceSlug,
  spec,
  flags,
  saved,
  effective,
  fallbackReason,
}: SurfacePickerRowProps): React.ReactElement | null {
  const actions = useModelSelectionActions();
  const [busy, setBusy] = React.useState(false);

  const match =
    effective === null ? null : effectiveChoiceFor(spec.choices, { saved, effective, fallback_reason: fallbackReason });
  if (spec.choices.length === 0) return null;
  const value = match === null ? "" : mergedChoiceKey(match);

  const notice =
    saved !== null && fallbackReason !== null ? fallbackNotice(saved.model, match?.label ?? "the default model") : null;

  const commit = async (key: string): Promise<void> => {
    const choice = spec.choices.find((row) => mergedChoiceKey(row) === key);
    if (choice === undefined || key === value) return;
    if (flags !== null && choice.origin === "catalog" && isBlockedCommit(choice.value, flags)) {
      notifyError(MODEL_PICKER_BLOCKED_TITLE, MODEL_PICKER_BLOCKED_BODY);
      return;
    }
    setBusy(true);
    try {
      await actions.save(spec.surface, { model: choice.value, connectionId: choice.connectionId ?? null });
      await actions.invalidate(workspaceSlug);
    } catch (error) {
      await actions.invalidate(workspaceSlug);
      notifyError(MODEL_PICKER_FAILED_TITLE, selectionFailureDetail(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-(--space-1)">
      <label className="text-body flex flex-wrap items-center gap-(--space-2) text-(--text)">
        <span className="min-w-36 font-medium">{spec.label}</span>
        <select
          data-testid={`surface-model-picker-${spec.surface}`}
          aria-label={spec.label}
          value={value}
          disabled={busy}
          onChange={(event) => void commit(event.target.value)}
          className="text-body h-(--control-height) min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
        >
          {match === null ? <option value="">{SURFACE_MODEL_EMPTY_OPTION}</option> : null}
          {spec.choices.map((choice) => (
            <option key={mergedChoiceKey(choice)} value={mergedChoiceKey(choice)} disabled={!choice.selectable}>
              {pickerOptionLabel(choice, choice.value === OPENHUB_DEFAULT_MODEL)}
            </option>
          ))}
        </select>
      </label>
      {notice === null ? null : (
        <p data-testid={`surface-model-fallback-${spec.surface}`} className="text-meta text-(--text-muted)">
          {notice}
        </p>
      )}
    </div>
  );
}

/** The Settings label for a surface. */
export function surfacePickerLabel(surface: AssistantSurface): string {
  switch (surface) {
    case "web_assistant":
      return WEB_ASSISTANT_LABEL;
    case "desktop_assistant":
      return DESKTOP_ASSISTANT_LABEL;
    case "agent":
      return AGENT_LABEL;
  }
}
