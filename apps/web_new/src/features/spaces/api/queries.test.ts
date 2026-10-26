// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  readSpaceViewer,
  resolveLegacySpaceAnchor,
  spaceGroupQueryOptions,
  spaceInstanceQueryOptions,
  spaceIssueDetailQueryOptions,
  spaceCommentsQueryOptions,
  spaceIssuesQueryOptions,
  spaceLabelsQueryOptions,
  spaceMembersQueryOptions,
  spaceMetaQueryOptions,
  spaceSettingsQueryOptions,
  spaceStatesQueryOptions,
  spaceViewerQueryOptions,
} from "./queries.js";

function transportOf(status: number, body: unknown): Transport {
  return async (url) => ({
    status,
    url,
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  });
}

function clientWith(transport: Transport) {
  return createClient({ baseUrl: "https://api.test", transport });
}

const UUID = "11111111-1111-4111-8111-111111111111";

describe("space query options", () => {
  it("probes the instance once with no retries", () => {
    const options = spaceInstanceQueryOptions(clientWith(transportOf(200, {})));
    expect(options.queryKey).toEqual(["spaces", "instance"]);
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });

  it("gates settings on a valid anchor and keys every read by anchor", () => {
    const client = clientWith(transportOf(200, {}));
    const valid = spaceSettingsQueryOptions(client, "anchor-1");
    expect(valid.queryKey).toEqual(["spaces", "anchor", "anchor-1", "settings"]);
    expect(valid.enabled).toBe(true);
    expect(spaceSettingsQueryOptions(client, "bad anchor").enabled).toBe(false);
    expect(spaceIssuesQueryOptions(client, "anchor-1").queryKey).toEqual([
      "spaces",
      "anchor",
      "anchor-1",
      "issues",
      "||",
    ]);
    expect(spaceStatesQueryOptions(client, "anchor-1").queryKey).toEqual(["spaces", "anchor", "anchor-1", "states"]);
    expect(spaceLabelsQueryOptions(client, "anchor-1").queryKey).toEqual(["spaces", "anchor", "anchor-1", "labels"]);
    expect(spaceMembersQueryOptions(client, "anchor-1").queryKey).toEqual(["spaces", "anchor", "anchor-1", "members"]);
  });

  it("never revalidates a board read on focus or reconnect (SPC-033)", () => {
    const client = clientWith(transportOf(200, {}));
    const options = [
      spaceInstanceQueryOptions(client),
      spaceSettingsQueryOptions(client, "anchor-1"),
      spaceIssuesQueryOptions(client, "anchor-1"),
      spaceIssueDetailQueryOptions(client, "anchor-1", UUID),
      spaceCommentsQueryOptions(client, "anchor-1", UUID),
      spaceGroupQueryOptions(client, "anchor-1", {}, UUID, "50:1:0"),
      spaceStatesQueryOptions(client, "anchor-1"),
      spaceLabelsQueryOptions(client, "anchor-1"),
      spaceMembersQueryOptions(client, "anchor-1"),
      spaceMetaQueryOptions(client, "anchor-1"),
      spaceViewerQueryOptions(client),
    ];
    for (const option of options) {
      expect(option.refetchOnWindowFocus).toBe(false);
      expect(option.refetchOnReconnect).toBe(false);
    }
  });

  it("reads one issue's detail for the overlay, gated on a uuid (SPC-040/043)", async () => {
    const urls: string[] = [];
    const detail = {
      id: UUID,
      name: "Public issue",
      state_id: null,
      sort_order: 1,
      estimate_point: null,
      priority: "high",
      start_date: null,
      target_date: "2020-01-15",
      sequence_id: 7,
      project_id: UUID,
      parent_id: null,
      created_by: null,
      state__group: "unstarted",
      cycle_id: null,
      assignee_ids: [],
      label_ids: [],
      module_ids: [],
      vote_items: [],
      reaction_items: [],
      description_json: null,
      description_html: "<p>Words</p>",
      description_stripped: "Words",
      description_binary: null,
    };
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () => Promise.resolve(JSON.stringify(detail)),
      };
    });
    const options = spaceIssueDetailQueryOptions(client, "anchor-1", UUID);
    expect(options.queryKey).toEqual(["spaces", "anchor", "anchor-1", "detail", UUID]);
    expect(options.enabled).toBe(true);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    await expect(options.queryFn!({} as never)).resolves.toMatchObject({ name: "Public issue" });
    expect(new URL(urls[0] ?? "", "https://api.test").pathname).toBe(`/api/public/anchor/anchor-1/issues/${UUID}/`);
    expect(spaceIssueDetailQueryOptions(client, "anchor-1", "SPC-42").enabled).toBe(false);
    expect(spaceIssueDetailQueryOptions(client, "bad anchor", UUID).enabled).toBe(false);
  });

  it("reads the public comment thread, gated on the anchor and issue uuid (SPC-049/050)", async () => {
    const urls: string[] = [];
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () => Promise.resolve(JSON.stringify([])),
      };
    });
    const options = spaceCommentsQueryOptions(client, "anchor-1", UUID);
    expect(options.queryKey).toEqual(["spaces", "anchor", "anchor-1", "comments", UUID]);
    expect(options.enabled).toBe(true);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    await expect(options.queryFn!({} as never)).resolves.toEqual([]);
    expect(new URL(urls[0] ?? "", "https://api.test").pathname).toBe(
      `/api/public/anchor/anchor-1/issues/${UUID}/comments/`
    );
    expect(spaceCommentsQueryOptions(client, "anchor-1", "SPC-42").enabled).toBe(false);
    expect(spaceCommentsQueryOptions(client, "bad anchor", UUID).enabled).toBe(false);
  });

  it("reads labels once per visit through the public anchor endpoint (SPC-028)", async () => {
    const urls: string[] = [];
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () => Promise.resolve(JSON.stringify([{ id: UUID, name: "Bug", color: "#e11d48", parent: null }])),
      };
    });
    const options = spaceLabelsQueryOptions(client, "anchor-1");
    expect(options.enabled).toBe(true);
    expect(options.staleTime).toBe(5 * 60_000);
    await expect(options.queryFn!({} as never)).resolves.toEqual([
      { id: UUID, name: "Bug", color: "#e11d48", parent: null },
    ]);
    expect(new URL(urls[0] ?? "", "https://api.test").pathname).toBe("/api/public/anchor/anchor-1/labels/");
    expect(spaceLabelsQueryOptions(client, "bad anchor").enabled).toBe(false);
  });

  it("reads board members once per visit through the public anchor endpoint (SPC-054)", async () => {
    const urls: string[] = [];
    const member = { id: UUID, member: UUID, member__display_name: "Ada L", member__avatar: "" };
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () => Promise.resolve(JSON.stringify([member])),
      };
    });
    const options = spaceMembersQueryOptions(client, "anchor-1");
    expect(options.enabled).toBe(true);
    expect(options.staleTime).toBe(5 * 60_000);
    await expect(options.queryFn!({} as never)).resolves.toEqual([member]);
    expect(new URL(urls[0] ?? "", "https://api.test").pathname).toBe("/api/public/anchor/anchor-1/members/");
    expect(spaceMembersQueryOptions(client, "bad anchor").enabled).toBe(false);
  });

  it("keys board metadata by anchor and never requests a malformed one", async () => {
    const meta = { name: "Public board", description: "Team roadmap" };
    const valid = spaceMetaQueryOptions(clientWith(transportOf(200, meta)), "anchor-1");
    expect(valid.queryKey).toEqual(["spaces", "anchor", "anchor-1", "meta"]);
    expect(valid.enabled).toBe(true);
    expect(valid.retry).toBe(false);
    await expect(valid.queryFn!({} as never)).resolves.toMatchObject(meta);
    expect(spaceMetaQueryOptions(clientWith(transportOf(200, meta)), "bad anchor").enabled).toBe(false);
  });

  it("requests the board's first grouped page with the address filters (SPC-026/036)", async () => {
    const urls: string[] = [];
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () =>
          Promise.resolve(
            JSON.stringify({
              grouped_by: "state_id",
              total_count: 0,
              next_cursor: null,
              next_page_results: false,
              results: {},
            })
          ),
      };
    });
    const state = "11111111-1111-4111-8111-111111111111";
    await spaceIssuesQueryOptions(client, "anchor-1", { state: [state] }).queryFn!({} as never);
    await spaceIssuesQueryOptions(client, "anchor-1", {}).queryFn!({} as never);
    expect(urls).toHaveLength(2);
    const filtered = new URL(urls[0] ?? "", "https://api.test");
    expect(filtered.pathname).toBe("/api/public/anchor/anchor-1/issues/");
    expect(filtered.searchParams.get("group_by")).toBe("state_id");
    expect(filtered.searchParams.get("per_page")).toBe("50");
    expect(filtered.searchParams.get("cursor")).toBe("50:0:0");
    expect(filtered.searchParams.get("order_by")).toBe("-created_at");
    expect(filtered.searchParams.get("state")).toBe(state);
    expect(filtered.searchParams.get("priority")).toBeNull();
    const all = new URL(urls[1] ?? "", "https://api.test");
    expect(all.searchParams.get("state")).toBeNull();
  });

  it("keys a later group page by anchor, state, filters and cursor", async () => {
    const urls: string[] = [];
    const client = clientWith(async (url) => {
      urls.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: () =>
          Promise.resolve(
            JSON.stringify({
              grouped_by: "state_id",
              total_count: 0,
              next_cursor: null,
              next_page_results: false,
              results: {},
            })
          ),
      };
    });
    const state = "11111111-1111-4111-8111-111111111111";
    const options = spaceGroupQueryOptions(client, "anchor-1", { priority: ["high"] }, state, "50:1:0");
    expect(options.queryKey).toEqual(["spaces", "anchor", "anchor-1", "issues", "|high|", "group", state, "50:1:0"]);
    await options.queryFn!({} as never);
    const url = new URL(urls[0] ?? "", "https://api.test");
    expect(url.searchParams.get("cursor")).toBe("50:1:0");
    expect(url.searchParams.get("state")).toBe(state);
    expect(url.searchParams.get("priority")).toBe("high");
    // Grouped stays on so a single group's page parses like the first one.
    expect(url.searchParams.get("group_by")).toBe("state_id");
  });

  it("reads the viewer as anonymous on 401 and on any other failure", async () => {
    await expect(readSpaceViewer(clientWith(transportOf(401, { detail: "no session" })))).resolves.toBeNull();
    await expect(readSpaceViewer(clientWith(transportOf(500, { detail: "down" })))).resolves.toBeNull();
  });

  it("keys the viewer read under the public root and resolves anonymously", async () => {
    const options = spaceViewerQueryOptions(clientWith(transportOf(401, { detail: "no session" })));
    expect(options.queryKey).toEqual(["spaces", "viewer"]);
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(60_000);
    await expect(options.queryFn!({} as never)).resolves.toBeNull();
  });

  it("returns the signed-in viewer", async () => {
    const me = {
      id: UUID,
      email: "ada@example.com",
      username: "ada",
      first_name: "Ada",
      last_name: "Lovelace",
      display_name: "Ada Lovelace",
      avatar: "",
      avatar_url: null,
      cover_image: null,
      cover_image_url: null,
      date_joined: "2026-01-01T00:00:00Z",
      user_timezone: "UTC",
      is_active: true,
      is_bot: false,
      is_email_verified: true,
      is_password_autoset: false,
      last_login_medium: "password",
      last_login_time: null,
    };
    await expect(readSpaceViewer(clientWith(transportOf(200, me)))).resolves.toMatchObject({
      email: "ada@example.com",
    });
  });
});

describe("resolveLegacySpaceAnchor", () => {
  const settings = {
    id: UUID,
    project_details: null,
    workspace_detail: { name: "Acme", slug: "acme", id: UUID, logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    created_by: null,
    updated_by: null,
    workspace: UUID,
    project: UUID,
    entity_identifier: "PROJ",
    entity_name: "project",
    anchor: "anchor-1",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: {},
    is_disabled: false,
    intake: null,
  };

  it("returns a valid project anchor", async () => {
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(200, settings)), "acme", UUID)).resolves.toBe(
      "anchor-1"
    );
  });

  it("refuses a non-project entity", async () => {
    await expect(
      resolveLegacySpaceAnchor(clientWith(transportOf(200, { ...settings, entity_name: "page" })), "acme", UUID)
    ).resolves.toBeNull();
  });

  it("refuses a missing or malformed anchor", async () => {
    for (const anchor of [null, "", "bad anchor", "a/b"]) {
      await expect(
        resolveLegacySpaceAnchor(clientWith(transportOf(200, { ...settings, anchor })), "acme", UUID)
      ).resolves.toBeNull();
    }
  });

  it("turns every server refusal into a generic miss", async () => {
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(404, {})), "acme", UUID)).resolves.toBeNull();
    await expect(resolveLegacySpaceAnchor(clientWith(transportOf(500, {})), "acme", UUID)).resolves.toBeNull();
  });
});
