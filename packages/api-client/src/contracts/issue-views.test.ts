// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view contracts (parity/specs/views.md §2, NEWFRONT-718): the
// issues saved-view route reads one project or workspace view back
// through the same record schema the create/update calls return; the
// views area lists both scopes and stars views through the generic
// favorites collection.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  addViewFavorite,
  createProjectView,
  createWorkspaceView,
  deleteProjectView,
  deleteWorkspaceView,
  getProjectView,
  getWorkspaceView,
  listProjectViews,
  listWorkspaceViews,
  removeViewFavorite,
  updateProjectView,
  updateWorkspaceView,
} from "./issue-views.js";

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

/** The server's stand-in for a missing view: a blank record with no id. */
const MISSING_ROW = {
  deleted_at: null,
  name: "",
  description: "",
  filters: null,
  display_filters: null,
  display_properties: null,
  rich_filters: null,
  sort_order: null,
  logo_props: null,
  archived_at: null,
  created_by: null,
  updated_by: null,
};

interface Call {
  method: string;
  url: string;
  body: string | undefined;
}

interface StubOptions {
  json?: unknown;
  query?: Record<string, string | number | boolean | undefined | null>;
}

/** The client's own query serialization, so assertions see the live URL. */
function withQuery(url: string, query: StubOptions["query"]): string {
  if (!query) return url;
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null) search.append(key, String(value));
  }
  const encoded = search.toString();
  return encoded ? `${url}?${encoded}` : url;
}

function stubClient(respond: (call: Call) => unknown): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const record = (method: string, url: string, options: StubOptions) => {
    const call = {
      method,
      url: withQuery(url, options.query),
      body: options.json === undefined ? undefined : JSON.stringify(options.json),
    };
    calls.push(call);
    return call;
  };
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string, options: StubOptions = {}) =>
      respond(record("GET", url, options))) as ApiClient["getJson"],
    sendJson: (async (method: string, url: string, options: StubOptions = {}) => {
      const call = record(method, url, options);
      return { data: respond(call), status: 200, url: call.url };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.push({ method: "DELETE", url, body: undefined });
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }) as ApiClient["delete"],
  } as ApiClient;
  return { client, calls };
}

describe("getProjectView", () => {
  it("reads the project view route and parses the record", async () => {
    const { client, calls } = stubClient(() => VIEW_ROW);
    const view = await getProjectView(client, "acme", UUID2, UUID);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `GET /api/workspaces/acme/projects/${UUID2}/views/${UUID}/`,
    ]);
    expect(view.name).toBe("Triage");
    expect(view.rich_filters).toEqual({ and: [{ state_group__in: "unstarted" }] });
  });

  it("reports the blank 200 record as a 404 instead of a phantom view", async () => {
    const { client } = stubClient(() => MISSING_ROW);
    const failure = await getProjectView(client, "acme", UUID2, UUID).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
    expect((failure as ApiError).code).toBe("not_found");
  });

  it("reports a null 200 body as a 404", async () => {
    const { client } = stubClient(() => null);
    await expect(getProjectView(client, "acme", UUID2, UUID)).rejects.toMatchObject({ status: 404 });
  });

  it("keeps an optional cloud anchor when the record carries one", async () => {
    const { client } = stubClient(() => ({ ...VIEW_ROW, anchor: "public-view-anchor" }));
    const view = await getProjectView(client, "acme", UUID2, UUID);
    expect(view.anchor).toBe("public-view-anchor");
  });
});

describe("getWorkspaceView", () => {
  it("reads the workspace view route and parses the record", async () => {
    const { client, calls } = stubClient(() => ({ ...VIEW_ROW, project: null }));
    const view = await getWorkspaceView(client, "acme", UUID);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([`GET /api/workspaces/acme/views/${UUID}/`]);
    expect(view.project).toBeNull();
  });

  it("reports the blank 200 record as a 404", async () => {
    const { client } = stubClient(() => MISSING_ROW);
    await expect(getWorkspaceView(client, "acme", UUID)).rejects.toMatchObject({ status: 404 });
  });
});

describe("listProjectViews", () => {
  it("reads the project view list and parses every record", async () => {
    const { client, calls } = stubClient(() => [VIEW_ROW, { ...VIEW_ROW, id: UUID2, project: null }]);
    const views = await listProjectViews(client, "acme", UUID2);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `GET /api/workspaces/acme/projects/${UUID2}/views/`,
    ]);
    expect(views.map((view) => view.id)).toEqual([UUID, UUID2]);
  });

  it("forwards the fields and order_by query keys", async () => {
    const { client, calls } = stubClient(() => []);
    await listProjectViews(client, "acme", UUID2, { fields: "id,name", orderBy: "-name" });
    expect(calls[0]?.url).toBe(`/api/workspaces/acme/projects/${UUID2}/views/?fields=id%2Cname&order_by=-name`);
  });
});

describe("listWorkspaceViews", () => {
  it("reads the workspace list and keeps the custom-only rows", async () => {
    const { client, calls } = stubClient(() => [{ ...VIEW_ROW, project: null }]);
    const views = await listWorkspaceViews(client, "acme", { orderBy: "-created_at" });
    expect(calls[0]?.url).toBe("/api/workspaces/acme/views/?order_by=-created_at");
    expect(views).toHaveLength(1);
    expect(views[0]?.project).toBeNull();
  });
});

describe("view favorites", () => {
  const FAVORITE = {
    id: UUID,
    entity_type: "view",
    entity_identifier: UUID2,
    entity_data: { id: UUID2, name: "Triage" },
    name: "Triage",
    is_folder: false,
    sequence: 65535,
    parent: null,
    workspace_id: UUID,
    project_id: UUID,
  };

  it("adds a favorite through the generic collection with the view pointer", async () => {
    const { client, calls } = stubClient(() => FAVORITE);
    const favorite = await addViewFavorite(client, "acme", { id: UUID2, name: "Triage", project: UUID });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual(["POST /api/workspaces/acme/user-favorites/"]);
    expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({
      entity_type: "view",
      entity_identifier: UUID2,
      project_id: UUID,
      name: "Triage",
    });
    expect(favorite.entity_type).toBe("view");
  });

  it("removes by the favorite row's id, not the view id", async () => {
    const { client, calls } = stubClient(() => ({}));
    await removeViewFavorite(client, "acme", UUID);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `DELETE /api/workspaces/acme/user-favorites/${UUID}/`,
    ]);
  });
});

describe("view writes", () => {
  it("creates, patches and deletes a project view on its own route", async () => {
    const { client, calls } = stubClient(() => VIEW_ROW);
    await createProjectView(client, "acme", UUID2, { name: "Triage", rich_filters: {}, logo_props: {} });
    await updateProjectView(client, "acme", UUID2, UUID, { name: "Triage edited", logo_props: { in_use: true } });
    await deleteProjectView(client, "acme", UUID2, UUID);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `POST /api/workspaces/acme/projects/${UUID2}/views/`,
      `PATCH /api/workspaces/acme/projects/${UUID2}/views/${UUID}/`,
      `DELETE /api/workspaces/acme/projects/${UUID2}/views/${UUID}/`,
    ]);
    expect(JSON.parse(calls[1]?.body ?? "{}")).toEqual({ name: "Triage edited", logo_props: { in_use: true } });
  });

  it("creates, patches and deletes a workspace view on its own route", async () => {
    const { client, calls } = stubClient(() => ({ ...VIEW_ROW, project: null }));
    await createWorkspaceView(client, "acme", { name: "Triage" });
    await updateWorkspaceView(client, "acme", UUID, { display_filters: { layout: "calendar" } });
    await deleteWorkspaceView(client, "acme", UUID);
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      "POST /api/workspaces/acme/views/",
      `PATCH /api/workspaces/acme/views/${UUID}/`,
      `DELETE /api/workspaces/acme/views/${UUID}/`,
    ]);
    expect(JSON.parse(calls[1]?.body ?? "{}")).toEqual({ display_filters: { layout: "calendar" } });
  });
});
