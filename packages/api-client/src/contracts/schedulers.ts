// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scheduler contracts: workspace scheduler-definition CRUD, project
// scheduler-binding (install) CRUD, the per-binding run history page and
// the project firing-calendar occurrences window. Run rows reuse the
// runners area's AgentRun schema (same serializer, same envelope), and
// the install dialogs' pod selector reads through the runners area's
// pod list — neither is re-contracted here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { AgentRunPage } from "./runs.js";

export const SCHEDULER_SOURCES = ["builtin", "manifest"] as const;

/**
 * One reusable scheduler definition: a URL handle plus the base prompt
 * every install fires with. `active_binding_count` is the live install
 * count; `description` is blank unless set.
 */
export const Scheduler = z.object({
  id: uuid(),
  workspace: uuid(),
  slug: z.string(),
  name: z.string(),
  description: z.string(),
  prompt: z.string(),
  color: z.string(),
  source: z.enum(SCHEDULER_SOURCES),
  is_enabled: z.boolean(),
  active_binding_count: z.number(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type Scheduler = z.infer<typeof Scheduler>;

export const SchedulerList = z.array(Scheduler);
export type SchedulerList = z.infer<typeof SchedulerList>;

/** GET /api/workspaces/<slug>/schedulers/ — every definition, any member. */
export async function listSchedulers(client: ApiClient, slug: string): Promise<SchedulerList> {
  const path = `/api/workspaces/${slug}/schedulers/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SchedulerList, body, `GET ${path}`);
}

/** GET .../schedulers/<id>/ — one definition (workspace admin). */
export async function getScheduler(client: ApiClient, slug: string, schedulerId: string): Promise<Scheduler> {
  const path = `/api/workspaces/${slug}/schedulers/${schedulerId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(Scheduler, body, `GET ${path}`);
}

export interface SchedulerCreateInput {
  name: string;
  slug: string;
  description?: string;
  prompt: string;
  color?: string;
  is_enabled?: boolean;
}

/**
 * POST .../schedulers/ answers 201 with the created row. Workspace admin
 * only; the handle must be unique per workspace and the color a 7-char
 * hex string.
 */
export async function createScheduler(
  client: ApiClient,
  slug: string,
  input: SchedulerCreateInput
): Promise<Scheduler> {
  const path = `/api/workspaces/${slug}/schedulers/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(Scheduler, envelope.data, `POST ${path}`);
}

export interface SchedulerUpdateInput {
  name?: string;
  description?: string;
  prompt?: string;
  color?: string;
  is_enabled?: boolean;
}

/**
 * PATCH .../schedulers/<id>/ answers 200 with the updated row. The
 * handle is immutable after creation, so it is not part of the input.
 */
export async function updateScheduler(
  client: ApiClient,
  slug: string,
  schedulerId: string,
  input: SchedulerUpdateInput
): Promise<Scheduler> {
  const path = `/api/workspaces/${slug}/schedulers/${schedulerId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(Scheduler, envelope.data, `PATCH ${path}`);
}

/** DELETE .../schedulers/<id>/ answers 204; resolves the status. */
export async function deleteScheduler(client: ApiClient, slug: string, schedulerId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/schedulers/${schedulerId}/`);
  return response.status;
}

export const OUTCOME_MODES = ["create_issue", "apply_fix", "fix_and_review"] as const;

/**
 * One project install of a scheduler. `dtstart` anchors the series and an
 * empty `rrule` means a single firing at `dtstart`; `pod` is null when
 * the install uses the project's default pod at fire time.
 * `last_run_status` / `last_run_ended_at` mirror the last AgentRun row
 * (null before the first firing); `next_run_at` goes stale while the
 * install is disabled.
 */
export const SchedulerBinding = z.object({
  id: uuid(),
  scheduler: uuid(),
  scheduler_slug: z.string(),
  scheduler_name: z.string(),
  scheduler_color: z.string(),
  project: uuid(),
  workspace: uuid(),
  dtstart: dateTime(),
  tzid: z.string(),
  rrule: z.string(),
  rdates: z.array(z.string()),
  exdates: z.array(z.string()),
  extra_context: z.string(),
  enabled: z.boolean(),
  outcome_mode: z.enum(OUTCOME_MODES),
  pod: nullableUuid(),
  pod_name: z.nullable(z.string()),
  next_run_at: nullableDateTime(),
  last_run: nullableUuid(),
  last_run_status: z.nullable(z.string()),
  last_run_ended_at: nullableDateTime(),
  last_error: z.string(),
  actor: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type SchedulerBinding = z.infer<typeof SchedulerBinding>;

export const SchedulerBindingList = z.array(SchedulerBinding);
export type SchedulerBindingList = z.infer<typeof SchedulerBindingList>;

/**
 * The install-detail shape: the list row plus the composed prompt the
 * dispatched agent receives, the total run count, and the parent
 * definition's source and enable flag. GET detail and PATCH both answer
 * this shape; only POST create answers the list shape.
 */
export const SchedulerBindingDetail = z.extend(SchedulerBinding, {
  resolved_prompt: z.string(),
  run_count: z.number(),
  scheduler_source: z.enum(SCHEDULER_SOURCES),
  scheduler_is_enabled: z.boolean(),
  /**
   * The API's exhausted-series signal (main#559 target). Absent while the
   * server does not report it, which is every build before that lands;
   * the detail page only badges Completed when the field says true.
   */
  series_exhausted: z.optional(z.boolean()),
});
export type SchedulerBindingDetail = z.infer<typeof SchedulerBindingDetail>;

/** GET .../projects/<id>/scheduler-bindings/ — the installs, any member. */
export async function listSchedulerBindings(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<SchedulerBindingList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SchedulerBindingList, body, `GET ${path}`);
}

/** GET .../scheduler-bindings/<bindingId>/ — detail shape, any member. */
export async function getSchedulerBinding(
  client: ApiClient,
  slug: string,
  projectId: string,
  bindingId: string
): Promise<SchedulerBindingDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/${bindingId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SchedulerBindingDetail, body, `GET ${path}`);
}

export interface SchedulerBindingCreateInput {
  scheduler: string;
  dtstart: string;
  tzid?: string;
  rrule?: string;
  rdates?: string[];
  exdates?: string[];
  extra_context?: string;
  enabled?: boolean;
  outcome_mode?: (typeof OUTCOME_MODES)[number];
  pod?: string | null;
}

/**
 * POST .../scheduler-bindings/ answers 201 with the created row in the
 * list shape. Project admin only; the definition must be enabled and the
 * pod (when set) must belong to the project. The project key travels in
 * the payload too — the serializer requires it even though the view pins
 * the same project from the URL at save time.
 */
export async function createSchedulerBinding(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: SchedulerBindingCreateInput
): Promise<SchedulerBinding> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { ...input, project: projectId } });
  return client.parse(SchedulerBinding, envelope.data, `POST ${path}`);
}

export interface SchedulerBindingUpdateInput {
  dtstart?: string;
  tzid?: string;
  rrule?: string;
  rdates?: string[];
  exdates?: string[];
  extra_context?: string;
  enabled?: boolean;
  outcome_mode?: (typeof OUTCOME_MODES)[number];
  pod?: string | null;
}

/**
 * PATCH .../scheduler-bindings/<bindingId>/ answers 200 in the detail
 * shape (the detail page reuses the response). The scheduler and project
 * links are locked server-side — repointing means uninstall + reinstall —
 * so they are not part of the input. An `{ enabled }`-only payload is
 * the list/detail enable switch.
 */
export async function updateSchedulerBinding(
  client: ApiClient,
  slug: string,
  projectId: string,
  bindingId: string,
  input: SchedulerBindingUpdateInput
): Promise<SchedulerBindingDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/${bindingId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(SchedulerBindingDetail, envelope.data, `PATCH ${path}`);
}

/** DELETE .../scheduler-bindings/<bindingId>/ answers 204; uninstalls. */
export async function deleteSchedulerBinding(
  client: ApiClient,
  slug: string,
  projectId: string,
  bindingId: string
): Promise<number> {
  const response = await client.delete(
    `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/${bindingId}/`
  );
  return response.status;
}

/**
 * GET .../scheduler-bindings/<bindingId>/runs/?page=&per_page=. A
 * dedicated endpoint (not a filter on the runners run list) because
 * that list's issue-centric visibility rules would hide scheduler runs
 * from project admins. Rows parse as the shared AgentRun page —
 * `count` is this page's length, `total_count` the binding's total.
 * Out-of-range `page` / `per_page` fall back to safe defaults
 * server-side rather than erroring.
 */
export async function listBindingRuns(
  client: ApiClient,
  slug: string,
  projectId: string,
  bindingId: string,
  paging: { page?: number; per_page?: number } = {}
): Promise<AgentRunPage> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/${bindingId}/runs/`;
  const body = await client.getJson<unknown>(path, { query: { ...paging } });
  return client.parse(AgentRunPage, body, `GET ${path}`);
}

export const OCCURRENCE_KINDS = ["scheduled", "past"] as const;

/**
 * One calendar occurrence: a future RRULE expansion (`scheduled`, no
 * run attached) or a past AgentRun in the window (`past`, with the run
 * id and status).
 */
export const SchedulerOccurrence = z.object({
  binding_id: uuid(),
  scheduler_id: uuid(),
  scheduler_name: z.string(),
  scheduler_color: z.string(),
  dtstart: dateTime(),
  tzid: z.string(),
  kind: z.enum(OCCURRENCE_KINDS),
  agent_run_id: nullableUuid(),
  status: z.nullable(z.string()),
});
export type SchedulerOccurrence = z.infer<typeof SchedulerOccurrence>;

/**
 * The occurrences window. The server caps the merged list at 5000 rows;
 * `has_more` with a `next_window_start` pointer marks truncation. A
 * window wider than 90 days is rejected with a 400.
 */
export const SchedulerOccurrenceWindow = z.object({
  occurrences: z.array(SchedulerOccurrence),
  has_more: z.boolean(),
  next_window_start: nullableDateTime(),
});
export type SchedulerOccurrenceWindow = z.infer<typeof SchedulerOccurrenceWindow>;

/**
 * GET .../scheduler-bindings/occurrences/?from=&to=. Both bounds
 * default server-side (previous through next 30 days) when omitted.
 */
export async function listSchedulerOccurrences(
  client: ApiClient,
  slug: string,
  projectId: string,
  window: { from?: string; to?: string } = {}
): Promise<SchedulerOccurrenceWindow> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/scheduler-bindings/occurrences/`;
  const body = await client.getJson<unknown>(path, { query: { ...window } });
  return client.parse(SchedulerOccurrenceWindow, body, `GET ${path}`);
}
