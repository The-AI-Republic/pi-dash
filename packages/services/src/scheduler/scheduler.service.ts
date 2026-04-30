/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "../api.service";

/**
 * Project Scheduler API client.
 *
 * Two surfaces:
 *
 * - **Definitions** under ``/api/workspaces/<slug>/schedulers/`` — list /
 *   create / update / soft-delete the prompt + cron template. Workspace
 *   admin only for mutations; any workspace member may list (so the
 *   project-side install picker can populate).
 * - **Bindings** under ``/api/workspaces/<slug>/projects/<id>/scheduler-bindings/``
 *   — install / edit / uninstall on a specific project. Project admin only.
 *
 * See ``.ai_design/project_scheduler/design.md`` §7.
 */

export type SchedulerSource = "builtin" | "manifest";

export interface IScheduler {
  id: string;
  workspace: string;
  slug: string;
  name: string;
  description: string;
  prompt: string;
  source: SchedulerSource;
  is_enabled: boolean;
  active_binding_count: number;
  created_at: string;
  updated_at: string;
}

export interface ISchedulerCreatePayload {
  slug: string;
  name: string;
  description?: string;
  prompt: string;
  is_enabled?: boolean;
}

export type ISchedulerUpdatePayload = Partial<Pick<IScheduler, "name" | "description" | "prompt" | "is_enabled">>;

export interface ISchedulerBinding {
  id: string;
  scheduler: string;
  scheduler_slug: string;
  scheduler_name: string;
  project: string;
  workspace: string;
  cron: string;
  extra_context: string;
  enabled: boolean;
  next_run_at: string | null;
  last_run: string | null;
  last_run_status: string | null;
  last_run_ended_at: string | null;
  last_error: string;
  actor: string | null;
  created_at: string;
  updated_at: string;
}

export interface ISchedulerBindingCreatePayload {
  scheduler: string;
  cron: string;
  extra_context?: string;
  enabled?: boolean;
}

export type ISchedulerBindingUpdatePayload = Partial<Pick<ISchedulerBinding, "cron" | "extra_context" | "enabled">>;

export class SchedulerService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  // -------- Definitions --------

  async listSchedulers(workspaceSlug: string): Promise<IScheduler[]> {
    return this.get(`/api/workspaces/${workspaceSlug}/schedulers/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async retrieveScheduler(workspaceSlug: string, schedulerId: string): Promise<IScheduler> {
    return this.get(`/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async createScheduler(workspaceSlug: string, payload: ISchedulerCreatePayload): Promise<IScheduler> {
    return this.post(`/api/workspaces/${workspaceSlug}/schedulers/`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async updateScheduler(
    workspaceSlug: string,
    schedulerId: string,
    payload: ISchedulerUpdatePayload
  ): Promise<IScheduler> {
    return this.patch(`/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async destroyScheduler(workspaceSlug: string, schedulerId: string): Promise<void> {
    return this.delete(`/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`)
      .then(() => undefined)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  // -------- Bindings --------

  async listBindings(workspaceSlug: string, projectId: string): Promise<ISchedulerBinding[]> {
    return this.get(`/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async retrieveBinding(workspaceSlug: string, projectId: string, bindingId: string): Promise<ISchedulerBinding> {
    return this.get(`/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async createBinding(
    workspaceSlug: string,
    projectId: string,
    payload: ISchedulerBindingCreatePayload
  ): Promise<ISchedulerBinding> {
    return this.post(`/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async updateBinding(
    workspaceSlug: string,
    projectId: string,
    bindingId: string,
    payload: ISchedulerBindingUpdatePayload
  ): Promise<ISchedulerBinding> {
    return this.patch(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`,
      payload
    )
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async destroyBinding(workspaceSlug: string, projectId: string, bindingId: string): Promise<void> {
    return this.delete(`/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`)
      .then(() => undefined)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
