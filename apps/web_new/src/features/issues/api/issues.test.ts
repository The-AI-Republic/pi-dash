// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { createClient, type Transport } from "@pidash/api-client";
import { issueKeys } from "./keys.js";
import {
  descriptionVersionQueryOptions,
  descriptionVersionsQueryOptions,
  issueDetailQueryOptions,
  issuesQueryOptions,
} from "./queries.js";

function stubTransport(body: unknown): Transport {
  return async () => ({
    status: 200,
    url: "https://api.test/",
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
}

const ENVELOPE = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 0,
  next_cursor: "",
  prev_cursor: "",
  next_page_results: false,
  prev_page_results: false,
  count: 0,
  total_results: 0,
  results: [],
};

describe("issue keys", () => {
  it("scopes every key under the workspace", () => {
    expect(issueKeys.list("acme", "pid", { layout: "list" })[0]).toBe("ws");
    expect(issueKeys.list("acme", "pid", { layout: "list" })[1]).toBe("acme");
    expect(issueKeys.detail("acme", "iid")).toEqual(["ws", "acme", "issues", "detail", "iid"]);
  });

  it("separates projects and view states", () => {
    expect(issueKeys.list("acme", "a", { layout: "list" })).not.toEqual(
      issueKeys.list("acme", "b", { layout: "list" })
    );
    expect(issueKeys.list("acme", "a", { layout: "list" })).not.toEqual(
      issueKeys.list("acme", "a", { layout: "compact" })
    );
  });

  it("nests the version reads under the workspace issues scope", () => {
    expect(issueKeys.descriptionVersions("acme", "pid", "iid")).toEqual([
      "ws",
      "acme",
      "issues",
      "description-versions",
      "pid",
      "iid",
    ]);
    expect(issueKeys.descriptionVersion("acme", "pid", "iid", "vid")).toEqual([
      ...issueKeys.descriptionVersions("acme", "pid", "iid"),
      "vid",
    ]);
  });
});

describe("issues query", () => {
  it("reads the list through the workspace-scoped URL", async () => {
    const seen: string[] = [];
    const inner = stubTransport(ENVELOPE);
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        seen.push(url);
        return inner(url, init);
      },
      validate: true,
    });
    const options = issuesQueryOptions(client, "acme", "pid", { layout: "list" });
    expect(options.queryKey[0]).toBe("ws");
    const data = await new QueryClient().fetchQuery(options);
    expect(data.total_count).toBe(0);
    expect(seen[0]).toBe("https://api.test/api/workspaces/acme/projects/pid/issues/?per_page=100&order_by=sort_order");
  });

  it("sends the stored order in the request and the key", async () => {
    const seen: string[] = [];
    const inner = stubTransport(ENVELOPE);
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        seen.push(url);
        return inner(url, init);
      },
      validate: true,
    });
    const options = issuesQueryOptions(client, "acme", "pid", { layout: "list" }, {}, "-created_at");
    await new QueryClient().fetchQuery(options);
    expect(seen[0]).toBe("https://api.test/api/workspaces/acme/projects/pid/issues/?per_page=100&order_by=-created_at");
    expect(options.queryKey).toEqual([...issueKeys.list("acme", "pid", { layout: "list" }, "-created_at"), null]);
    expect(options.queryKey).not.toEqual([...issueKeys.list("acme", "pid", { layout: "list" }, "sort_order"), null]);
  });

  it("sends the stored rich expression as the filters parameter", async () => {
    const seen: string[] = [];
    const inner = stubTransport(ENVELOPE);
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        seen.push(url);
        return inner(url, init);
      },
      validate: true,
    });
    const options = issuesQueryOptions(
      client,
      "acme",
      "pid",
      { layout: "list" },
      {
        and: [{ priority__in: "urgent" }, { not: { state_id__in: ["a"] } }],
      }
    );
    expect(options.queryKey).toContain(
      JSON.stringify({ and: [{ priority__in: "urgent" }, { not: { state_id__in: ["a"] } }] })
    );
    await new QueryClient().fetchQuery(options);
    expect(seen[0]).toContain(
      `filters=${encodeURIComponent(JSON.stringify({ and: [{ priority__in: "urgent" }, { not: { state_id__in: ["a"] } }] }))}`
    );
    expect(seen[0]).toContain("per_page=100");
    expect(seen[0]).toContain("order_by=sort_order");
  });
});

const DETAIL_ROW = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  name: "Fix login",
  state_id: null,
  sort_order: 1,
  completed_at: null,
  estimate_point: null,
  priority: "high",
  sequence_id: 42,
  project_id: "223e4567-e89b-12d3-a456-426614174001",
  parent_id: null,
  cycle_id: null,
  module_ids: [],
  label_ids: [],
  assignee_ids: [],
  sub_issues_count: null,
  attachment_count: null,
  link_count: null,
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
  is_draft: false,
  archived_at: null,
  complexity_score: 0,
  agent_executor: null,
  is_synced: false,
  description_html: "<p>old</p>",
  is_subscribed: false,
  agent_ticker: null,
  agent_status: null,
  relations_summary: { blocked_by: [], blocking: [] },
  has_open_blockers: false,
};

const VERSION_ROW = {
  id: "323e4567-e89b-12d3-a456-426614174002",
  workspace: "423e4567-e89b-12d3-a456-426614174003",
  project: "223e4567-e89b-12d3-a456-426614174001",
  issue: "123e4567-e89b-12d3-a456-426614174000",
  last_saved_at: "2024-02-03T04:05:06Z",
  owned_by: "523e4567-e89b-12d3-a456-426614174004",
  created_at: "2024-02-03T04:05:06Z",
  updated_at: "2024-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
};

const VERSIONS_ENVELOPE = {
  prev_cursor: "",
  cursor: "",
  next_cursor: null,
  prev_page_results: false,
  next_page_results: false,
  page_count: 1,
  total_results: 1,
  total_pages: 1,
  results: [VERSION_ROW],
};

const VERSION_DETAIL = {
  ...VERSION_ROW,
  description_binary: null,
  description_html: "<p>restored</p>",
  description_stripped: "restored",
  description_json: {},
};

describe("detail and version queries", () => {
  it("hydrates the detail without focus revalidation", async () => {
    const seen: string[] = [];
    const inner = stubTransport(DETAIL_ROW);
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        seen.push(url);
        return inner(url, init);
      },
      validate: true,
    });
    const options = issueDetailQueryOptions(
      client,
      "acme",
      "223e4567-e89b-12d3-a456-426614174001",
      "123e4567-e89b-12d3-a456-426614174000"
    );
    expect(options.queryKey).toEqual(issueKeys.detail("acme", "123e4567-e89b-12d3-a456-426614174000"));
    expect(options.refetchOnWindowFocus).toBe(false);
    const data = await new QueryClient().fetchQuery(options);
    expect(data.name).toBe("Fix login");
    expect(seen[0]).toBe(
      "https://api.test/api/workspaces/acme/projects/223e4567-e89b-12d3-a456-426614174001/issues/123e4567-e89b-12d3-a456-426614174000/"
    );
  });

  it("reads the version history and one version through the work-item URLs", async () => {
    const seen: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        seen.push(url);
        const body = url.includes("description-versions/323e4567") ? VERSION_DETAIL : VERSIONS_ENVELOPE;
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify(body),
        };
      },
      validate: true,
    });
    const queryClient = new QueryClient();
    const list = await queryClient.fetchQuery(
      descriptionVersionsQueryOptions(
        client,
        "acme",
        "223e4567-e89b-12d3-a456-426614174001",
        "123e4567-e89b-12d3-a456-426614174000"
      )
    );
    expect(list.results).toHaveLength(1);
    const one = await queryClient.fetchQuery(
      descriptionVersionQueryOptions(
        client,
        "acme",
        "223e4567-e89b-12d3-a456-426614174001",
        "123e4567-e89b-12d3-a456-426614174000",
        "323e4567-e89b-12d3-a456-426614174002"
      )
    );
    expect(one.description_html).toBe("<p>restored</p>");
    expect(seen[0]).toContain("/description-versions/");
    expect(seen[1]).toContain("/description-versions/323e4567-e89b-12d3-a456-426614174002/");
  });
});
