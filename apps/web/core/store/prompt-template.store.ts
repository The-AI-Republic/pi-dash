/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { set, unset } from "lodash-es";
import { action, computed, makeObservable, observable, runInAction } from "mobx";
import { computedFn } from "mobx-utils";
// pi dash imports
import type {
  IPromptTemplate,
  IPromptTemplateCreatePayload,
  IPromptTemplatePreviewResponse,
  IPromptTemplateUpdatePayload,
} from "@pi-dash/types";
import { PromptTemplateService } from "@pi-dash/services";
// store
import type { CoreRootStore } from "./root.store";

export interface IPromptTemplateStore {
  // observable
  templateMap: Record<string, IPromptTemplate>;
  fetchedMap: Record<string, boolean>;
  loader: boolean;
  // computed
  workspaceTemplates: IPromptTemplate[];
  // actions
  getTemplatesForWorkspace: (workspaceSlug: string) => IPromptTemplate[];
  getTemplateById: (templateId: string) => IPromptTemplate | null;
  getEffectiveTemplate: (workspaceSlug: string) => IPromptTemplate | null;
  fetchTemplates: (workspaceSlug: string) => Promise<IPromptTemplate[]>;
  createOverride: (workspaceSlug: string, payload?: IPromptTemplateCreatePayload) => Promise<IPromptTemplate>;
  updateTemplate: (
    workspaceSlug: string,
    templateId: string,
    payload: IPromptTemplateUpdatePayload
  ) => Promise<IPromptTemplate>;
  archiveTemplate: (workspaceSlug: string, templateId: string) => Promise<IPromptTemplate>;
  previewTemplate: (
    workspaceSlug: string,
    templateId: string,
    issueId: string,
    draftBody?: string
  ) => Promise<IPromptTemplatePreviewResponse>;
}

/**
 * Holds the prompt templates visible to the *currently routed* workspace (the
 * global default plus any workspace-scoped override). Mutating actions return
 * the updated row so callers can thread it through SWR / toasts without
 * refetching the list.
 */
export class PromptTemplateStore implements IPromptTemplateStore {
  // observable
  templateMap: Record<string, IPromptTemplate> = {};
  fetchedMap: Record<string, boolean> = {};
  loader = false;
  // root
  rootStore: CoreRootStore;
  // service
  promptTemplateService: PromptTemplateService;

  constructor(_rootStore: CoreRootStore) {
    makeObservable(this, {
      templateMap: observable,
      fetchedMap: observable,
      loader: observable,
      workspaceTemplates: computed,
      fetchTemplates: action,
      createOverride: action,
      updateTemplate: action,
      archiveTemplate: action,
    });
    this.rootStore = _rootStore;
    this.promptTemplateService = new PromptTemplateService();
  }

  get workspaceTemplates(): IPromptTemplate[] {
    const slug = this.rootStore.router.workspaceSlug;
    if (!slug) return [];
    return this.getTemplatesForWorkspace(slug);
  }

  getTemplatesForWorkspace = computedFn((workspaceSlug: string): IPromptTemplate[] => {
    const workspace = this.rootStore.workspaceRoot.getWorkspaceBySlug(workspaceSlug);
    if (!workspace) return [];
    const filtered = Object.values(this.templateMap).filter(
      (t) => t.is_active && (t.workspace === workspace.id || t.workspace === null)
    );
    // Copy first, then sort — the filter output is a fresh array but
    // keeping the spread explicit makes this obvious to readers, and
    // oxlint's `no-array-sort` rule no longer flags a mutating sort on a
    // not-obviously-owned value.
    return [...filtered].sort((a, b) => {
      // Workspace-scoped override first, then the global default, so the UI
      // displays "your customization" ahead of the fallback.
      if (a.workspace && !b.workspace) return -1;
      if (!a.workspace && b.workspace) return 1;
      return a.name.localeCompare(b.name);
    });
  });

  getTemplateById = computedFn((templateId: string): IPromptTemplate | null => this.templateMap[templateId] ?? null);

  getEffectiveTemplate = computedFn((workspaceSlug: string): IPromptTemplate | null => {
    const templates = this.getTemplatesForWorkspace(workspaceSlug);
    if (templates.length === 0) return null;
    // First entry is the workspace override if present, else the global default.
    return templates[0] ?? null;
  });

  fetchTemplates = async (workspaceSlug: string): Promise<IPromptTemplate[]> => {
    this.loader = true;
    try {
      const response = await this.promptTemplateService.list(workspaceSlug);
      runInAction(() => {
        response.forEach((t) => set(this.templateMap, [t.id], t));
        set(this.fetchedMap, workspaceSlug, true);
      });
      return response;
    } finally {
      runInAction(() => {
        this.loader = false;
      });
    }
  };

  createOverride = async (
    workspaceSlug: string,
    payload: IPromptTemplateCreatePayload = {}
  ): Promise<IPromptTemplate> => {
    const response = await this.promptTemplateService.create(workspaceSlug, payload);
    runInAction(() => {
      set(this.templateMap, [response.id], response);
    });
    return response;
  };

  updateTemplate = async (
    workspaceSlug: string,
    templateId: string,
    payload: IPromptTemplateUpdatePayload
  ): Promise<IPromptTemplate> => {
    const response = await this.promptTemplateService.update(workspaceSlug, templateId, payload);
    runInAction(() => {
      set(this.templateMap, [response.id], response);
    });
    return response;
  };

  archiveTemplate = async (workspaceSlug: string, templateId: string): Promise<IPromptTemplate> => {
    const response = await this.promptTemplateService.archive(workspaceSlug, templateId);
    // Archived rows must drop out of the active list so the UI falls back to
    // the global default.
    runInAction(() => {
      unset(this.templateMap, templateId);
    });
    return response;
  };

  previewTemplate = async (
    workspaceSlug: string,
    templateId: string,
    issueId: string,
    draftBody?: string
  ): Promise<IPromptTemplatePreviewResponse> =>
    this.promptTemplateService.preview(workspaceSlug, templateId, {
      issue_id: issueId,
      ...(draftBody ? { body: draftBody } : {}),
    });
}
