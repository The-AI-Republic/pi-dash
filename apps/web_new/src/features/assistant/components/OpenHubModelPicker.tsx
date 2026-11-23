// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Merged model picker (NEWPIDASH-394, design sections 7.1 and 7.5). A
// compact dropdown in the composer's bottom bar, ChatGPT/Claude style:
// catalog models first (god-mode order with server labels), then BYOK
// models grouped by key. Choosing a model writes that surface's saved
// choice through PUT model-selection/{surface} — the desktop build
// writes desktop_assistant, the browser writes web_assistant — then
// refreshes the selection so Settings shows the same value. It renders
// whenever the surface has a choice, on every lane; a saved choice the
// server no longer honors shows the fallback notice. A native <select>
// never submits the composer form.

import { cn } from "@pidash/kit";
import * as React from "react";

import type { AssistantSurface } from "@pidash/api-client";

import {
  connectionModelFlags,
  isBlockedCommit,
  useCloudConnections,
  useCloudQuota,
} from "../../cloud-assistant/index.js";
import { notifyError } from "../../../shared/shell/index.js";

import {
  composerSurface,
  selectionFailureDetail,
  useModelSelection,
  useModelSelectionActions,
} from "../api/selection.js";
import {
  ASSISTANT_MODEL_PICKER_LABEL,
  MODEL_PICKER_BLOCKED_BODY,
  MODEL_PICKER_BLOCKED_TITLE,
  MODEL_PICKER_FAILED_TITLE,
} from "../lib/copy.js";
import {
  applyCreditBadges,
  buildMergedModelChoices,
  byokKeysFromList,
  catalogFromList,
  effectiveChoiceFor,
  fallbackNotice,
  mergedChoiceKey,
  OPENHUB_DEFAULT_MODEL,
  pickerOptionLabel,
} from "../lib/openhubPicker.js";

export interface OpenHubModelPickerProps {
  /** Workspace scoping the selection read the picker refreshes. */
  workspaceSlug: string;
  /** The surface this composer writes; defaults to the build's surface. */
  surface?: AssistantSurface | undefined;
  className?: string;
}

export function OpenHubModelPicker({
  workspaceSlug,
  surface: surfaceProp,
  className,
}: OpenHubModelPickerProps): React.ReactElement | null {
  const surface = surfaceProp ?? composerSurface();
  const connections = useCloudConnections();
  const quota = useCloudQuota();
  const selection = useModelSelection(workspaceSlug);
  const actions = useModelSelectionActions();
  const [busy, setBusy] = React.useState(false);

  const list = connections.data;
  const state = selection.data?.[surface] ?? null;
  if (list === undefined || state === null) return null;

  const flags = connectionModelFlags(list, quota.data ?? null);
  const merged = buildMergedModelChoices(catalogFromList(list), byokKeysFromList(list));
  if (merged.length === 0 || state.effective === null) return null;

  const choices = applyCreditBadges(merged, flags);
  const effective = effectiveChoiceFor(choices, state);
  if (effective === null) return null;
  const effectiveKey = mergedChoiceKey(effective);

  const notice =
    state.saved !== null && state.fallback_reason ? fallbackNotice(state.saved.model, effective.label) : null;

  const commit = async (key: string): Promise<void> => {
    const choice = choices.find((row) => mergedChoiceKey(row) === key);
    if (choice === undefined || key === effectiveKey) return;
    if (choice.origin === "catalog" && isBlockedCommit(choice.value, flags)) {
      notifyError(MODEL_PICKER_BLOCKED_TITLE, MODEL_PICKER_BLOCKED_BODY);
      return;
    }
    setBusy(true);
    try {
      await actions.save(surface, { model: choice.value, connectionId: choice.connectionId ?? null });
      // The selection read is the picker's value and notice; refresh it
      // and the composer's config read so Settings agrees, too.
      await actions.invalidate(workspaceSlug);
    } catch (error) {
      // A slug the server rejects (removed since the list loaded)
      // refetches the selection and keeps the old value: the controlled
      // value stays effectiveKey until the refetch answers.
      await actions.invalidate(workspaceSlug);
      notifyError(MODEL_PICKER_FAILED_TITLE, selectionFailureDetail(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-(--space-1)">
      <select
        data-testid="assistant-model-picker"
        aria-label={ASSISTANT_MODEL_PICKER_LABEL}
        value={effectiveKey}
        disabled={busy}
        onChange={(event) => void commit(event.target.value)}
        className={cn(
          "text-body max-w-56 truncate rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)",
          className
        )}
      >
        {choices.map((choice) => (
          <option key={mergedChoiceKey(choice)} value={mergedChoiceKey(choice)} disabled={!choice.selectable}>
            {pickerOptionLabel(choice, choice.value === OPENHUB_DEFAULT_MODEL)}
          </option>
        ))}
      </select>
      {notice === null ? null : (
        <p data-testid="assistant-model-fallback" className="text-meta text-(--text-muted)">
          {notice}
        </p>
      )}
    </div>
  );
}
