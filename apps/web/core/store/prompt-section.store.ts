/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { action, makeObservable, observable, runInAction } from "mobx";
// pi dash imports
import type {
  IPromptCompiledResponse,
  IPromptPreviewPayload,
  IPromptPreviewResponse,
  IPromptSectionListResponse,
  IPromptSectionOverride,
  IPromptSectionUpsertPayload,
  TPromptKind,
  TPromptScope,
} from "@pi-dash/types";
import { PromptSectionService } from "@pi-dash/services";
// store
import type { CoreRootStore } from "./root.store";

export interface IPromptSectionStore {
  // observable
  loader: boolean;
  // actions
  fetchSections: (workspaceSlug: string, kind: TPromptKind, scope: TPromptScope) => Promise<IPromptSectionListResponse>;
  fetchCompiled: (workspaceSlug: string, kind: TPromptKind, scope: TPromptScope) => Promise<IPromptCompiledResponse>;
  upsertSection: (
    workspaceSlug: string,
    sectionKey: string,
    payload: IPromptSectionUpsertPayload
  ) => Promise<IPromptSectionOverride>;
  revertSection: (workspaceSlug: string, sectionKey: string, scope: TPromptScope) => Promise<void>;
  previewPrompt: (
    workspaceSlug: string,
    kind: TPromptKind,
    payload: IPromptPreviewPayload
  ) => Promise<IPromptPreviewResponse>;
}

/**
 * Thin store over the prompt-section REST surface. The page caches list /
 * compiled responses with SWR, so this store stays stateless beyond a shared
 * loader flag; the mutating actions return the server row for toast threading.
 */
export class PromptSectionStore implements IPromptSectionStore {
  loader = false;
  rootStore: CoreRootStore;
  promptSectionService: PromptSectionService;

  constructor(_rootStore: CoreRootStore) {
    makeObservable(this, {
      loader: observable,
      fetchSections: action,
      fetchCompiled: action,
      upsertSection: action,
      revertSection: action,
    });
    this.rootStore = _rootStore;
    this.promptSectionService = new PromptSectionService();
  }

  fetchSections = async (
    workspaceSlug: string,
    kind: TPromptKind,
    scope: TPromptScope
  ): Promise<IPromptSectionListResponse> => {
    this.loader = true;
    try {
      return await this.promptSectionService.list(workspaceSlug, kind, scope);
    } finally {
      runInAction(() => {
        this.loader = false;
      });
    }
  };

  fetchCompiled = async (
    workspaceSlug: string,
    kind: TPromptKind,
    scope: TPromptScope
  ): Promise<IPromptCompiledResponse> => this.promptSectionService.compiled(workspaceSlug, kind, scope);

  upsertSection = async (
    workspaceSlug: string,
    sectionKey: string,
    payload: IPromptSectionUpsertPayload
  ): Promise<IPromptSectionOverride> => this.promptSectionService.upsert(workspaceSlug, sectionKey, payload);

  revertSection = async (workspaceSlug: string, sectionKey: string, scope: TPromptScope): Promise<void> =>
    this.promptSectionService.revert(workspaceSlug, sectionKey, scope);

  previewPrompt = async (
    workspaceSlug: string,
    kind: TPromptKind,
    payload: IPromptPreviewPayload
  ): Promise<IPromptPreviewResponse> => this.promptSectionService.preview(workspaceSlug, kind, payload);
}
