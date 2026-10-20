// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared fixtures for the relations and links widget suites. Not a test
// file itself: the vitest include only picks *.test.ts(x).

import { QueryClient } from "@tanstack/react-query";
import type { IssueLink, IssueRelations, RelatedIssue } from "@pidash/api-client";

export const WS = "acme";
export const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
export const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
export const PEER = "423e4567-e89b-12d3-a456-426614174003";
export const LINK = "523e4567-e89b-12d3-a456-426614174004";

export interface TransportCall {
  method: string;
  url: string;
  body: string;
}

export function relatedRow(overrides: Partial<RelatedIssue> = {}): RelatedIssue {
  return {
    id: PEER,
    name: "Peer work item",
    state_id: null,
    sort_order: 1,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    label_ids: [],
    assignee_ids: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    relation_type: "blocked_by",
    ...overrides,
  };
}

export function emptyRelations(): IssueRelations {
  return {
    blocking: [],
    blocked_by: [],
    duplicate: [],
    relates_to: [],
    start_after: [],
    start_before: [],
    finish_after: [],
    finish_before: [],
  };
}

export function linkRow(overrides: Partial<IssueLink> = {}): IssueLink {
  return {
    id: LINK,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    created_by: null,
    updated_by: null,
    deleted_at: null,
    project: PROJECT,
    workspace: "623e4567-e89b-12d3-a456-426614174005",
    title: "Design doc",
    url: "https://example.com/doc",
    issue: ISSUE,
    metadata: {},
    created_by_detail: {
      id: "723e4567-e89b-12d3-a456-426614174006",
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      is_bot: false,
      display_name: "Ada L",
    },
    ...overrides,
  };
}

export function newQueryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

export interface StubOptions {
  relations?: IssueRelations;
  links?: IssueLink[];
  candidates?: Array<Record<string, unknown>>;
  writeStatus?: number;
  relationsStatus?: number;
  linksStatus?: number;
  /** Detail read for the related row; true marks it an epic. */
  peerIsEpic?: boolean;
  candidatesStatus?: number;
  peerStatus?: number;
  extraProjects?: Array<{ id: string; identifier: string }>;
  peerDelayMs?: number;
}

/** Route the widget's reads and writes; every call is recorded. */
export function stubTransport(options: StubOptions = {}) {
  const calls: TransportCall[] = [];
  const transport = async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ method, url, body: init.body ?? "" });
    let status = 200;
    let payload: unknown = {};
    if (url.endsWith("/auth/get-csrf-token/")) {
      payload = { csrf_token: "csrf" };
    } else if (url.includes("/search-issues/")) {
      status = options.candidatesStatus ?? 200;
      payload = options.candidates ?? [];
    } else if (url.includes("/issue-relation/") || url.includes("/remove-relation/")) {
      if (method === "GET") {
        status = options.relationsStatus ?? 200;
        payload = options.relations ?? emptyRelations();
      } else {
        status = options.writeStatus ?? (method === "POST" && url.includes("/issue-relation/") ? 201 : 204);
        payload = method === "POST" && url.includes("/issue-relation/") ? [] : "";
      }
    } else if (url.endsWith("/projects/")) {
      payload = [
        ...(options.extraProjects ?? []).map((project) => ({
          id: project.id,
          name: project.identifier,
          identifier: project.identifier,
          sort_order: 1,
          logo_props: {},
          member_role: 20,
          intake_count: 0,
          archived_at: null,
          workspace: "623e4567-e89b-12d3-a456-426614174005",
          cycle_view: true,
          issue_views_view: true,
          module_view: true,
          page_view: true,
          inbox_view: true,
          is_default: false,
          guest_view_all_features: false,
          project_lead: null,
          network: 2,
          created_at: "2024-01-01T00:00:00Z",
          updated_at: "2024-01-01T00:00:00Z",
          created_by: null,
          updated_by: null,
        })),
        {
          id: PROJECT,
          name: "Web",
          identifier: "WEB",
          sort_order: 1,
          logo_props: {},
          member_role: 20,
          intake_count: 0,
          archived_at: null,
          workspace: "623e4567-e89b-12d3-a456-426614174005",
          cycle_view: true,
          issue_views_view: true,
          module_view: true,
          page_view: true,
          inbox_view: true,
          is_default: true,
          guest_view_all_features: false,
          project_lead: null,
          network: 2,
          created_at: "2024-01-01T00:00:00Z",
          updated_at: "2024-01-01T00:00:00Z",
          created_by: null,
          updated_by: null,
        },
      ];
    } else if (url.includes("/issue-links/")) {
      if (method === "GET") {
        status = options.linksStatus ?? 200;
        payload = options.links ?? [];
      } else if (method === "DELETE") {
        status = options.writeStatus ?? 204;
        payload = "";
      } else {
        status = options.writeStatus ?? (method === "POST" ? 201 : 200);
        payload = options.links?.[0] ?? linkRow();
      }
    } else if (method === "GET" && url.endsWith(`/issues/${PEER}/`)) {
      if (options.peerDelayMs !== undefined) await new Promise((resolve) => setTimeout(resolve, options.peerDelayMs));
      status = options.peerStatus ?? 200;
      payload =
        status >= 400
          ? { detail: "gone" }
          : {
              id: PEER,
              name: "Peer work item",
              state_id: null,
              sort_order: 1,
              completed_at: null,
              estimate_point: null,
              priority: "high",
              start_date: null,
              target_date: null,
              sequence_id: 42,
              project_id: PROJECT,
              parent_id: null,
              cycle_id: null,
              module_ids: [],
              label_ids: [],
              assignee_ids: [],
              sub_issues_count: 0,
              attachment_count: null,
              link_count: 0,
              created_at: "2024-01-01T00:00:00Z",
              updated_at: "2024-01-01T00:00:00Z",
              created_by: null,
              updated_by: null,
              is_draft: false,
              is_epic: options.peerIsEpic ?? false,
              is_intake: false,
              archived_at: null,
              complexity_score: 0,
              agent_executor: null,
              is_synced: false,
              description_html: "<p></p>",
              is_subscribed: false,
              agent_ticker: null,
              agent_status: null,
              relations_summary: { blocked_by: [], blocking: [] },
              has_open_blockers: false,
            };
    } else if (url.includes("/issues/")) {
      status = options.writeStatus ?? (method === "DELETE" ? 204 : 204);
      payload = options.writeStatus === undefined ? "" : { detail: "boom" };
    }
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (typeof payload === "string" ? payload : JSON.stringify(payload)),
    };
  };
  return { calls, transport };
}
