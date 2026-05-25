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
  /** 7-char hex like "#3b82f6". Used to color this scheduler's calendar blocks. */
  color: string;
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
  color?: string;
  is_enabled?: boolean;
}

export type ISchedulerUpdatePayload = Partial<
  Pick<IScheduler, "name" | "description" | "prompt" | "color" | "is_enabled">
>;

export interface ISchedulerBinding {
  id: string;
  scheduler: string;
  scheduler_slug: string;
  scheduler_name: string;
  /** Joined from Scheduler.color so the calendar can render without a second fetch. */
  scheduler_color: string;
  project: string;
  workspace: string;
  /** ISO datetime. Series anchor for the RRULE expansion. */
  dtstart: string;
  /** IANA tz name. Currently informational (expansion runs in UTC); future PRs add wall-clock semantics. */
  tzid: string;
  /** RFC 5545 RRULE string. Empty = single-shot at dtstart. */
  rrule: string;
  /** Extra one-off firings (ISO strings). */
  rdates: string[];
  /** Firings to skip (ISO strings). */
  exdates: string[];
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
  /**
   * Project the binding installs on. Redundant with the projectId already
   * encoded in the request URL, but the cloud serializer's ``is_valid()``
   * runs before its view-level ``serializer.save(project=...)`` override
   * and rejects the body if this isn't present. Until the backend stops
   * requiring it, the client has to send it.
   */
  project: string;
  dtstart: string;
  tzid?: string;
  rrule: string;
  rdates?: string[];
  exdates?: string[];
  extra_context?: string;
  enabled?: boolean;
}

export type ISchedulerBindingUpdatePayload = Partial<
  Pick<ISchedulerBinding, "dtstart" | "tzid" | "rrule" | "rdates" | "exdates" | "extra_context" | "enabled">
>;

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

  // -------- Occurrences (calendar) --------

  async listOccurrences(
    workspaceSlug: string,
    projectId: string,
    params: { from?: string; to?: string } = {}
  ): Promise<ISchedulerOccurrenceResponse> {
    const search = new URLSearchParams();
    if (params.from) search.set("from", params.from);
    if (params.to) search.set("to", params.to);
    const qs = search.toString();
    const url = `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/occurrences/${qs ? `?${qs}` : ""}`;
    return this.get(url)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}

// ---------------------------------------------------------------------------
// Occurrences (calendar)
// ---------------------------------------------------------------------------

export type SchedulerOccurrenceKind = "scheduled" | "past";

/** One occurrence on the project calendar — past run or future firing. */
export interface ISchedulerOccurrence {
  binding_id: string;
  scheduler_id: string;
  scheduler_name: string;
  /** Joined from Scheduler.color so the calendar renders without a second fetch. */
  scheduler_color: string;
  /** UTC ISO datetime — the firing instant. */
  dtstart: string;
  tzid: string;
  /** "scheduled" = expanded from RRULE; "past" = a real AgentRun row. */
  kind: SchedulerOccurrenceKind;
  /** Present when kind="past". */
  agent_run_id: string | null;
  /** Present when kind="past" — the AgentRun status. */
  status: string | null;
}

export interface ISchedulerOccurrenceResponse {
  occurrences: ISchedulerOccurrence[];
  has_more: boolean;
  next_window_start: string | null;
}
