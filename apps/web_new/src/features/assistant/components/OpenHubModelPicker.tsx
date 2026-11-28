// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub model picker (NEWPIDASH-303). A compact dropdown that lives in
// the composer's bottom bar next to send, ChatGPT/Claude style, instead
// of only in Settings. It lists the enabled OpenHub models in admin
// order (the god-mode list via openhub_model_options, NEWPIDASH-302) with
// the current model selected and the free model marked as the default.
// Choosing a model writes through the same PATCH connection-detail
// endpoint Settings uses, then refreshes the connections and both
// config reads so Settings shows the same value. It renders only for an
// OpenHub lane with access; BYOK/Codex controls stay in Settings. A
// native <select> never submits the composer form.
//
// The LLM config arrives as a prop from the composer host (which already
// reads it), never as a second observer of the same query: the host
// gates its pending branch on that read, so a mount-time refetch here
// would flip the host back to its spinner, unmount the picker, and loop.

import { cn } from "@pidash/kit";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import type { LlmConfig } from "@pidash/api-client";

import {
  actionFailureDetail,
  annotateChoices,
  connectionModelFlags,
  isBlockedCommit,
  modelChoices,
  OPENHUB_MODEL_DESCRIPTIONS,
  useCloudAssistantActions,
  useCloudConnections,
  useCloudQuota,
} from "../../cloud-assistant/index.js";
import { notifyError } from "../../../shared/shell/index.js";

import { assistantKeys } from "../api/keys.js";
import {
  ASSISTANT_MODEL_PICKER_LABEL,
  MODEL_PICKER_BLOCKED_BODY,
  MODEL_PICKER_BLOCKED_TITLE,
  MODEL_PICKER_FAILED_FALLBACK,
  MODEL_PICKER_FAILED_TITLE,
} from "../lib/copy.js";
import { OPENHUB_DEFAULT_MODEL, pickerOptionLabel, selectedOpenHubConnection } from "../lib/openhubPicker.js";

export interface OpenHubModelPickerProps {
  /** Workspace whose assistant config read gates the lane. */
  workspaceSlug: string;
  /** The host's config read; absent disables the picker. */
  config: LlmConfig | undefined;
  className?: string;
}

export function OpenHubModelPicker({
  workspaceSlug,
  config,
  className,
}: OpenHubModelPickerProps): React.ReactElement | null {
  const queryClient = useQueryClient();
  const connections = useCloudConnections();
  const quota = useCloudQuota();
  const actions = useCloudAssistantActions();
  const [busy, setBusy] = React.useState(false);

  const row = selectedOpenHubConnection(config, connections.data);
  if (row === null || connections.data === undefined) return null;

  const flags = connectionModelFlags(connections.data, quota.data ?? null);
  const choices = annotateChoices(
    modelChoices(flags.openhubModels, {}, OPENHUB_MODEL_DESCRIPTIONS, row.model_name),
    flags
  );
  if (choices.length === 0) return null;

  const commit = async (model: string): Promise<void> => {
    if (model === row.model_name) return;
    if (isBlockedCommit(model, flags)) {
      notifyError(MODEL_PICKER_BLOCKED_TITLE, MODEL_PICKER_BLOCKED_BODY);
      return;
    }
    setBusy(true);
    try {
      await actions.setModel(row.id, model);
      // actions.invalidate clears the connections list and the cloud
      // config read; the composer's own config read is cleared here so
      // Settings and the composer agree on the new value.
      await Promise.all([
        actions.invalidate(),
        queryClient.invalidateQueries({ queryKey: assistantKeys.config(workspaceSlug) }),
      ]);
    } catch (error) {
      // A slug the server rejects (removed since the list loaded) refetches
      // the list and keeps the old selection: the controlled value stays
      // row.model_name until the refetch answers.
      await actions.invalidate();
      notifyError(MODEL_PICKER_FAILED_TITLE, actionFailureDetail(error, MODEL_PICKER_FAILED_FALLBACK));
    } finally {
      setBusy(false);
    }
  };

  return (
    <select
      data-testid="assistant-model-picker"
      aria-label={ASSISTANT_MODEL_PICKER_LABEL}
      value={row.model_name}
      disabled={busy}
      onChange={(event) => void commit(event.target.value)}
      className={cn(
        "text-body max-w-56 truncate rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)",
        className
      )}
    >
      {choices.map((choice) => (
        <option key={choice.value} value={choice.value} disabled={!choice.selectable}>
          {pickerOptionLabel(choice, choice.value === OPENHUB_DEFAULT_MODEL)}
        </option>
      ))}
    </select>
  );
}
