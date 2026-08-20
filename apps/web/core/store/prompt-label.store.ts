/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import type {
  IPromptLabel,
  IPromptLabelAssignmentPayload,
  IPromptLabelCreatePayload,
  IPromptLabelListResponse,
  IPromptLabelUpdatePayload,
} from "@pi-dash/types";
import { PromptLabelService } from "@pi-dash/services";
// store
import type { CoreRootStore } from "./root.store";

export interface IPromptLabelStore {
  // actions
  fetchLabels: (workspaceSlug: string) => Promise<IPromptLabelListResponse>;
  createLabel: (workspaceSlug: string, payload: IPromptLabelCreatePayload) => Promise<IPromptLabel>;
  updateLabel: (workspaceSlug: string, labelId: string, payload: IPromptLabelUpdatePayload) => Promise<IPromptLabel>;
  deleteLabel: (workspaceSlug: string, labelId: string) => Promise<void>;
  attachLabel: (
    workspaceSlug: string,
    labelId: string,
    payload: IPromptLabelAssignmentPayload
  ) => Promise<IPromptLabel>;
  detachLabel: (workspaceSlug: string, labelId: string, payload: IPromptLabelAssignmentPayload) => Promise<void>;
}

/**
 * Thin store over the prompt-label REST surface. Mirrors {@link
 * PromptSectionStore}: the prompts page caches the label list with SWR and
 * drives its own state, so this store holds no observable state; the mutating
 * actions return the server row for optimistic UI / toast threading.
 */
export class PromptLabelStore implements IPromptLabelStore {
  rootStore: CoreRootStore;
  promptLabelService: PromptLabelService;

  constructor(_rootStore: CoreRootStore) {
    this.rootStore = _rootStore;
    this.promptLabelService = new PromptLabelService();
  }

  fetchLabels = async (workspaceSlug: string): Promise<IPromptLabelListResponse> =>
    this.promptLabelService.list(workspaceSlug);

  createLabel = async (workspaceSlug: string, payload: IPromptLabelCreatePayload): Promise<IPromptLabel> =>
    this.promptLabelService.create(workspaceSlug, payload);

  updateLabel = async (
    workspaceSlug: string,
    labelId: string,
    payload: IPromptLabelUpdatePayload
  ): Promise<IPromptLabel> => this.promptLabelService.update(workspaceSlug, labelId, payload);

  deleteLabel = async (workspaceSlug: string, labelId: string): Promise<void> =>
    this.promptLabelService.remove(workspaceSlug, labelId);

  attachLabel = async (
    workspaceSlug: string,
    labelId: string,
    payload: IPromptLabelAssignmentPayload
  ): Promise<IPromptLabel> => this.promptLabelService.attach(workspaceSlug, labelId, payload);

  detachLabel = async (workspaceSlug: string, labelId: string, payload: IPromptLabelAssignmentPayload): Promise<void> =>
    this.promptLabelService.detach(workspaceSlug, labelId, payload);
}
