// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the public board issue reads: the grouped envelope and
// the flat detail row, plus the query parameters the board sends.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import { getSpaceBoardIssue, listSpaceBoardIssues } from "./space-issues.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";

const row = {
  id: UUID,
  name: "Board card",
  state_id: UUID2,
  sort_order: 65535,
  estimate_point: null,
  priority: "medium",
  start_date: null,
  target_date: "2026-10-30",
  sequence_id: 4,
  project_id: UUID3,
  parent_id: null,
  created_by: UUID,
  state__group: "backlog",
  cycle_id: null,
  assignee_ids: [UUID],
  label_ids: [UUID2],
  module_ids: [],
  vote_items: [
    {
      vote: 1,
      actor_details: {
        id: UUID,
        display_name: "Tester",
        first_name: "T",
        last_name: "Tester",
        avatar: "",
        avatar_url: null,
      },
    },
  ],
  reaction_items: [
    {
      reaction: "1f600",
      actor_details: {
        id: UUID,
        display_name: "Tester",
        first_name: "T",
        last_name: "Tester",
        avatar: "",
        avatar_url: null,
      },
    },
  ],
};

const detail = {
  ...row,
  description_json: {},
  description_html: "<p>Body</p>",
  description_stripped: "Body",
  description_binary: null,
};

const page = {
  grouped_by: "state_id",
  sub_grouped_by: null,
  total_count: 1,
  next_cursor: "50:1:0",
  prev_cursor: "50:-1:1",
  next_page_results: false,
  prev_page_results: false,
  count: 1,
  total_pages: 1,
  total_results: 1,
  extra_stats: null,
  results: { [UUID2]: { results: [row], total_results: 1 } },
};

function clientFor(value: unknown) {
  const transport = recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ body: JSON.stringify(value) })
  );
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space issue contracts", () => {
  it("parses the grouped envelope keyed by state id", async () => {
    const { client, transport } = clientFor(page);
    const parsed = await listSpaceBoardIssues(client, "anchor123", {
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
      orderBy: "-created_at",
    });
    expect(parsed.grouped_by).toBe("state_id");
    expect(parsed.results[UUID2]?.total_results).toBe(1);
    expect(parsed.results[UUID2]?.results[0]?.vote_items[0]?.actor_details.display_name).toBe("Tester");
    const url = new URL(transport.calls.at(-1)?.url ?? "");
    expect(url.pathname).toBe("/api/public/anchor/anchor123/issues/");
    expect(url.searchParams.get("group_by")).toBe("state_id");
    expect(url.searchParams.get("per_page")).toBe("50");
    expect(url.searchParams.get("cursor")).toBe("50:0:0");
    expect(url.searchParams.get("order_by")).toBe("-created_at");
  });

  it("joins filter arrays with commas", async () => {
    const { client, transport } = clientFor(page);
    await listSpaceBoardIssues(client, "anchor123", {
      state: [UUID2, UUID3],
      priority: ["medium", "high"],
      labels: [UUID, UUID2],
    });
    const url = new URL(transport.calls.at(-1)?.url ?? "");
    expect(url.searchParams.get("state")).toBe(`${UUID2},${UUID3}`);
    expect(url.searchParams.get("priority")).toBe("medium,high");
    expect(url.searchParams.get("labels")).toBe(`${UUID},${UUID2}`);
  });

  it("omits empty filters from the query", async () => {
    const { client, transport } = clientFor(page);
    await listSpaceBoardIssues(client, "anchor123", { state: [], priority: [] });
    const url = new URL(transport.calls.at(-1)?.url ?? "");
    expect(url.searchParams.has("state")).toBe(false);
    expect(url.searchParams.has("priority")).toBe(false);
  });

  it("rejects an ungrouped list where the grouped record is due", async () => {
    const { client } = clientFor({ ...page, grouped_by: null, results: [row] });
    await expect(listSpaceBoardIssues(client, "anchor123")).rejects.toThrow();
  });

  it("parses the detail row with description fields", async () => {
    const { client, transport } = clientFor(detail);
    const parsed = await getSpaceBoardIssue(client, "anchor123", UUID);
    expect(parsed.description_html).toBe("<p>Body</p>");
    expect(parsed.reaction_items[0]?.reaction).toBe("1f600");
    expect(transport.calls.at(-1)?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID}/`);
  });

  it("rejects a row that drops the flat state id", async () => {
    const { client } = clientFor({ ...detail, state_id: undefined });
    await expect(getSpaceBoardIssue(client, "anchor123", UUID)).rejects.toThrow();
  });
});
