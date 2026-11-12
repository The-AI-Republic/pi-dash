// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recent-visit row models (SHELL-014–017; written from parity/specs/shell.md
// §2). The feed answers each visit with the entity's own serialized shape,
// or null when it is gone. These parsers narrow that untyped payload into
// the row each renderer needs: visits without a payload, without a title,
// or of an unknown kind are skipped, and missing fields degrade to null
// (their marker is omitted) instead of breaking the row.

import type { ProjectList, RecentVisit, RecentVisitList, StateList } from "@pidash/api-client";

export type RecentFilter = "all" | "issue" | "page" | "project";

export const RECENT_FILTER_LABELS: Record<RecentFilter, string> = {
  all: "All",
  issue: "Work items",
  page: "Pages",
  project: "Projects",
};

const FILTER_ORDER: RecentFilter[] = ["all", "issue", "page", "project"];

export const RECENT_FILTERS: Array<{ key: RecentFilter; label: string }> = FILTER_ORDER.map((key) => ({
  key,
  label: RECENT_FILTER_LABELS[key],
}));

/** The `entity_name` query value for a filter (omitted for All). */
export function recentsEntityParam(filter: RecentFilter): string | undefined {
  return filter === "all" ? undefined : filter;
}

export interface RecentIssueData {
  kind: "issue";
  title: string;
  issueId: string;
  projectId: string | null;
  sequenceId: number | null;
  projectIdentifier: string | null;
  stateId: string | null;
  stateNameHint: string | null;
  priority: string | null;
  assigneeCount: number;
  assigneeNames: string[];
}

export interface RecentPageData {
  kind: "page";
  title: string;
  pageId: string;
  projectId: string | null;
  projectIdentifier: string | null;
  hasOwner: boolean;
}

export interface RecentProjectData {
  kind: "project";
  title: string;
  projectId: string;
  identifier: string | null;
  memberCount: number;
}

export type RecentEntityData = RecentIssueData | RecentPageData | RecentProjectData;

export interface RecentRow {
  visitId: string;
  visitedAt: string;
  entity: RecentEntityData;
}

type Payload = Record<string, unknown>;

function isRecord(value: unknown): value is Payload {
  return typeof value === "object" && value !== null;
}

function readString(payload: Payload, key: string): string | null {
  const value = payload[key];
  return typeof value === "string" && value !== "" ? value : null;
}

function readNumber(payload: Payload, key: string): number | null {
  const value = payload[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function readAssignees(payload: Payload): { count: number; names: string[] } {
  const raw: unknown = payload["assignees"] ?? payload["assignee_ids"];
  if (!Array.isArray(raw)) return { count: 0, names: [] };
  const names: string[] = [];
  for (const entry of raw) {
    if (!isRecord(entry)) continue;
    const name: unknown = entry["display_name"] ?? entry["name"];
    if (typeof name === "string" && name !== "") names.push(name);
  }
  return { count: raw.length, names };
}

function readStateRef(payload: Payload): { id: string | null; name: string | null } {
  const raw: unknown = payload["state"] ?? payload["state_id"];
  if (typeof raw === "string") return { id: raw === "" ? null : raw, name: null };
  if (isRecord(raw)) return { id: readString(raw, "id"), name: readString(raw, "name") };
  return { id: null, name: null };
}

function readTitle(payload: Payload, entityIdentifier: string): string | null {
  const name = readString(payload, "name");
  if (name !== null) return name;
  return entityIdentifier === "" ? null : entityIdentifier;
}

function parseIssueRow(visit: RecentVisit, payload: Payload): RecentRow | null {
  const title = readTitle(payload, visit.entity_identifier);
  if (title === null) return null;
  const state = readStateRef(payload);
  const assignees = readAssignees(payload);
  return {
    visitId: visit.id,
    visitedAt: visit.visited_at,
    entity: {
      kind: "issue",
      title,
      issueId: readString(payload, "id") ?? visit.entity_identifier,
      projectId: readString(payload, "project_id"),
      sequenceId: readNumber(payload, "sequence_id"),
      projectIdentifier: readString(payload, "project_identifier"),
      stateId: state.id,
      stateNameHint: state.name,
      priority: readString(payload, "priority"),
      assigneeCount: assignees.count,
      assigneeNames: assignees.names,
    },
  };
}

function parsePageRow(visit: RecentVisit, payload: Payload): RecentRow | null {
  const title = readTitle(payload, visit.entity_identifier);
  if (title === null) return null;
  return {
    visitId: visit.id,
    visitedAt: visit.visited_at,
    entity: {
      kind: "page",
      title,
      pageId: readString(payload, "id") ?? visit.entity_identifier,
      projectId: readString(payload, "project_id"),
      projectIdentifier: readString(payload, "project_identifier"),
      hasOwner: readString(payload, "owned_by") !== null,
    },
  };
}

function parseProjectRow(visit: RecentVisit, payload: Payload): RecentRow | null {
  const title = readTitle(payload, visit.entity_identifier);
  if (title === null) return null;
  const members: unknown = payload["project_members"];
  return {
    visitId: visit.id,
    visitedAt: visit.visited_at,
    entity: {
      kind: "project",
      title,
      projectId: readString(payload, "id") ?? visit.entity_identifier,
      identifier: readString(payload, "identifier"),
      memberCount: Array.isArray(members) ? members.length : 0,
    },
  };
}

export function parseRecentRow(visit: RecentVisit): RecentRow | null {
  const payload = visit.entity_data;
  if (!isRecord(payload)) return null;
  switch (visit.entity_name) {
    case "issue":
      return parseIssueRow(visit, payload);
    case "page":
    case "workspace_page":
      return parsePageRow(visit, payload);
    case "project":
      return parseProjectRow(visit, payload);
    default:
      return null;
  }
}

/** Parses a feed page, skipping visits without a renderable row. Order is kept. */
export function parseRecentRows(visits: RecentVisitList): RecentRow[] {
  const rows: RecentRow[] = [];
  for (const visit of visits) {
    const row = parseRecentRow(visit);
    if (row !== null) rows.push(row);
  }
  return rows;
}

/** Work-item key line: "ABC-123" from inline fields, else the lone identifier. */
export function issueKeyLabel(issue: RecentIssueData, fallbackIdentifier: string | null): string | null {
  const identifier = issue.projectIdentifier ?? fallbackIdentifier;
  if (identifier !== null && issue.sequenceId !== null) return `${identifier}-${issue.sequenceId}`;
  return identifier;
}

/** Detail address for a work item (home when the project is unknown). */
export function issueDetailHref(workspaceSlug: string, projectId: string | null, issueId: string): string {
  if (projectId === null) return `/${workspaceSlug}/`;
  return `/${workspaceSlug}/projects/${projectId}/issues/${issueId}`;
}

/** Page address in the right scope: project pages stay in the project. */
export function pageHref(workspaceSlug: string, projectId: string | null, pageId: string): string {
  if (projectId === null) return `/${workspaceSlug}/pages/${pageId}`;
  return `/${workspaceSlug}/projects/${projectId}/pages/${pageId}`;
}

/** Project address: its work list. */
export function projectHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/issues`;
}

/** Distinct project ids across issue rows, for the states lookups. */
export function recentProjectIds(rows: RecentRow[]): string[] {
  const ids = new Set<string>();
  for (const row of rows) {
    if (row.entity.kind === "issue" && row.entity.projectId !== null) ids.add(row.entity.projectId);
  }
  return [...ids];
}

export interface ResolvedState {
  name: string;
  color: string;
}

/** Indexes states-list reads by id, skipping failed or pending entries. */
export function indexStateLists(lists: Array<StateList | undefined>): Map<string, ResolvedState> {
  const byId = new Map<string, ResolvedState>();
  for (const list of lists) {
    if (list === undefined) continue;
    for (const state of list) byId.set(state.id, { name: state.name, color: state.color });
  }
  return byId;
}

/** Status display for an issue row: the nested hint wins, else the states lookup. */
export function resolveIssueState(
  issue: RecentIssueData,
  statesById: Map<string, ResolvedState>
): { name: string | null; color: string | null } {
  if (issue.stateNameHint !== null) return { name: issue.stateNameHint, color: null };
  if (issue.stateId === null) return { name: null, color: null };
  const resolved = statesById.get(issue.stateId);
  if (resolved === undefined) return { name: null, color: null };
  return { name: resolved.name, color: resolved.color };
}

/** Indexes the project list by id for identifier fallbacks. */
export function indexProjectIdentifiers(projects: ProjectList | undefined): Map<string, string> {
  const byId = new Map<string, string>();
  if (projects === undefined) return byId;
  for (const project of projects) byId.set(project.id, project.identifier);
  return byId;
}
