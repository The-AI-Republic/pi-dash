/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { set, unset } from "lodash-es";
import { action, makeObservable, observable, runInAction } from "mobx";
import { computedFn } from "mobx-utils";
// pi dash imports
import type {
  IScheduler,
  ISchedulerCreatePayload,
  ISchedulerUpdatePayload,
} from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
// store
import type { CoreRootStore } from "./root.store";

export interface ISchedulerStore {
  // observable
  schedulerMap: Record<string, IScheduler>;
  fetchedMap: Record<string, boolean>;
  loader: boolean;
  // actions
  getSchedulersForWorkspace: (workspaceSlug: string) => IScheduler[];
  getSchedulerById: (schedulerId: string) => IScheduler | null;
  fetchSchedulers: (workspaceSlug: string) => Promise<IScheduler[]>;
  createScheduler: (workspaceSlug: string, payload: ISchedulerCreatePayload) => Promise<IScheduler>;
  updateScheduler: (
    workspaceSlug: string,
    schedulerId: string,
    payload: ISchedulerUpdatePayload
  ) => Promise<IScheduler>;
  deleteScheduler: (workspaceSlug: string, schedulerId: string) => Promise<void>;
}

/**
 * Workspace-scoped scheduler-definition store. Mirrors the shape of
 * `PromptTemplateStore` — single normalized map keyed by id, derived
 * per-workspace lists via a computedFn so SWR + MobX stay coherent.
 *
 * This store does NOT carry bindings (the per-project install rows). Bindings
 * live on the project surface and have their own lifecycle.
 */
export class SchedulerStore implements ISchedulerStore {
  // observable
  schedulerMap: Record<string, IScheduler> = {};
  fetchedMap: Record<string, boolean> = {};
  loader = false;
  // root
  rootStore: CoreRootStore;
  // service
  schedulerService: SchedulerService;

  constructor(_rootStore: CoreRootStore) {
    makeObservable(this, {
      schedulerMap: observable,
      fetchedMap: observable,
      loader: observable,
      fetchSchedulers: action,
      createScheduler: action,
      updateScheduler: action,
      deleteScheduler: action,
    });
    this.rootStore = _rootStore;
    this.schedulerService = new SchedulerService();
  }

  getSchedulersForWorkspace = computedFn((workspaceSlug: string): IScheduler[] => {
    const workspace = this.rootStore.workspaceRoot.getWorkspaceBySlug(workspaceSlug);
    if (!workspace) return [];
    const filtered = Object.values(this.schedulerMap).filter((s) => s.workspace === workspace.id);
    return [...filtered].toSorted((a, b) => a.name.localeCompare(b.name));
  });

  getSchedulerById = computedFn((schedulerId: string): IScheduler | null => this.schedulerMap[schedulerId] ?? null);

  fetchSchedulers = async (workspaceSlug: string): Promise<IScheduler[]> => {
    this.loader = true;
    try {
      const response = await this.schedulerService.listSchedulers(workspaceSlug);
      runInAction(() => {
        response.forEach((s) => set(this.schedulerMap, [s.id], s));
        set(this.fetchedMap, workspaceSlug, true);
      });
      return response;
    } finally {
      runInAction(() => {
        this.loader = false;
      });
    }
  };

  createScheduler = async (workspaceSlug: string, payload: ISchedulerCreatePayload): Promise<IScheduler> => {
    const response = await this.schedulerService.createScheduler(workspaceSlug, payload);
    runInAction(() => {
      set(this.schedulerMap, [response.id], response);
    });
    return response;
  };

  updateScheduler = async (
    workspaceSlug: string,
    schedulerId: string,
    payload: ISchedulerUpdatePayload
  ): Promise<IScheduler> => {
    const response = await this.schedulerService.updateScheduler(workspaceSlug, schedulerId, payload);
    runInAction(() => {
      set(this.schedulerMap, [response.id], response);
    });
    return response;
  };

  deleteScheduler = async (workspaceSlug: string, schedulerId: string): Promise<void> => {
    await this.schedulerService.destroyScheduler(workspaceSlug, schedulerId);
    runInAction(() => {
      unset(this.schedulerMap, schedulerId);
    });
  };
}
