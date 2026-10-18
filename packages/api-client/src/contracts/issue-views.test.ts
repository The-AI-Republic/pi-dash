// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view read contracts (ISS-104): the issues saved-view route reads
// one project or workspace view back through the same record schema the
// create/update calls return.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { getProjectView, getWorkspaceView } from "./issue-views.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const AT = "2026-10-16T15:32:05.479661+09:00";

const VIEW_ROW = {
  id: UUID,
  workspace: UUID,
  project: UUID2,
  name: "Triage",
  description: "",
  query: {},
  filters: {},
  display_filters: { layout: "list" },
  display_properties: {},
  rich_filters: { and: [{ state_group__in: "unstarted" }] },
  access: 0,
  sort_order: 1,
  logo_props: {},
  owned_by: UUID,
  is_locked: false,
  archived_at: null,
  is_favorite: false,
  created_by: UUID,
  updated_by: null,
  created_at: AT,
  updated_at: AT,
};

function stubClient(body: unknown): { client: ApiClient; urls: string[] } {
  const urls: string[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string) => {
      urls.push(url);
      return body;
    }) as ApiClient["getJson"],
  } as ApiClient;
  return { client, urls };
}

describe("getProjectView", () => {
  it("reads the project view route and parses the record", async () => {
    const { client, urls } = stubClient(VIEW_ROW);
    const view = await getProjectView(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/views/${UUID}/`]);
    expect(view.name).toBe("Triage");
    expect(view.rich_filters).toEqual({ and: [{ state_group__in: "unstarted" }] });
  });
});

describe("getWorkspaceView", () => {
  it("reads the workspace view route and parses the record", async () => {
    const { client, urls } = stubClient({ ...VIEW_ROW, project: null });
    const view = await getWorkspaceView(client, "acme", UUID);
    expect(urls).toEqual([`/api/workspaces/acme/views/${UUID}/`]);
    expect(view.project).toBeNull();
  });
});
