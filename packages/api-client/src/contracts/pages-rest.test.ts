// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub proofs for the core pages REST contracts (NEWFRONT-833). Each test
// pins the path and verb a caller emits, the request body where there is
// one, and the schema's reading of the serializer's shape. The live
// operations half lives in pages.test.ts; the live proof of this half is
// src/contract/pages.contract.test.ts.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  addPageFavorite,
  archivePage,
  createPage,
  deletePage,
  duplicatePage,
  getPage,
  getPageDescriptionBinary,
  getPageVersion,
  listPageVersions,
  listPages,
  lockPage,
  movePageToProject,
  removePageFavorite,
  restorePage,
  setPageAccess,
  unlockPage,
  updatePage,
  updatePageDescription,
} from "./pages.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PAGE = "123e4567-e89b-12d3-a456-426614174000";
const USER = "323e4567-e89b-12d3-a456-426614174002";
const VERSION = "423e4567-e89b-12d3-a456-426614174003";
const BASE = `/api/workspaces/${WS}/projects/${PROJECT}/pages/`;

interface Call {
  method: string;
  url: string;
  json?: unknown;
  query?: unknown;
}

const MARK = { in_use: "emoji" as const, emoji: { value: "1f600" } };

function stubClient(handlers: { json?: (call: Call) => unknown; raw?: (call: Call) => unknown } = {}): {
  client: ApiClient;
  calls: Call[];
} {
  const calls: Call[] = [];
  const respond = (call: Call, isGet: boolean) => {
    if (isGet) return handlers.raw?.(call) ?? handlers.json?.(call);
    return { data: handlers.json?.(call) ?? null, status: 200, url: call.url };
  };
  const record =
    (method: string) =>
    async (url: string, options: { json?: unknown; query?: unknown } = {}) => {
      const call = { method, url, json: options.json, query: options.query };
      calls.push(call);
      return respond(call, method === "GET");
    };
  const sendJson = (async (method: string, url: string, options: { json?: unknown; query?: unknown } = {}) => {
    const call = { method, url, json: options.json, query: options.query };
    calls.push(call);
    return respond(call, false);
  }) as ApiClient["sendJson"];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: record("GET") as ApiClient["getJson"],
    sendJson,
    get: record("GET") as ApiClient["get"],
    post: record("POST") as ApiClient["post"],
    patch: record("PATCH") as ApiClient["patch"],
    delete: record("DELETE") as ApiClient["delete"],
  } as ApiClient;
  return { client, calls };
}

/** A row exactly as the collection serializer projects it. */
function row(overrides: Record<string, unknown> = {}) {
  return {
    id: PAGE,
    name: "Meeting notes",
    owned_by: USER,
    access: 0,
    color: "",
    parent: null,
    is_favorite: false,
    is_locked: false,
    archived_at: null,
    workspace: "523e4567-e89b-12d3-a456-426614174004",
    created_at: "2026-10-20T10:00:00Z",
    updated_at: "2026-10-20T10:00:00Z",
    created_by: USER,
    updated_by: USER,
    view_props: { full_width: false },
    logo_props: MARK,
    label_ids: [],
    project_ids: [PROJECT],
    ...overrides,
  };
}

const DETAIL = row({ description_html: "<p>Body</p>", issue_ids: [] });
const CREATED = row({ description_html: "<p>Body</p>" });

describe("pages collection contracts", () => {
  it("reads the collection with no filter by default", async () => {
    const { client, calls } = stubClient({ json: () => [row()] });
    const rows = await listPages(client, WS, PROJECT);
    expect(calls[0]?.method).toBe("GET");
    expect(calls[0]?.url).toBe(BASE);
    expect(calls[0]?.query).toEqual({ parent: undefined, search: undefined });
    expect(rows[0]?.name).toBe("Meeting notes");
    expect(rows[0]?.parent).toBeNull();
  });

  it("passes parent and search for a children read", async () => {
    const { client, calls } = stubClient({ json: () => [] });
    await listPages(client, WS, PROJECT, { parent: PAGE, search: "budget" });
    expect(calls[0]?.query).toEqual({ parent: PAGE, search: "budget" });
  });

  it("parses the nested fields when the backend serves them", async () => {
    const { client } = stubClient({ json: () => [row({ parent: PAGE, parent_id: PAGE, sub_pages_count: 3 })] });
    const rows = await listPages(client, WS, PROJECT);
    expect(rows[0]?.parent_id).toBe(PAGE);
    expect(rows[0]?.sub_pages_count).toBe(3);
  });

  it("rejects a row with no id", async () => {
    const { client } = stubClient({ json: () => [{ name: "No id" }] });
    await expect(listPages(client, WS, PROJECT)).rejects.toThrow();
  });

  it("reads one page and asks not to track the visit", async () => {
    const { client, calls } = stubClient({ json: () => DETAIL });
    const detail = await getPage(client, WS, PROJECT, PAGE, { trackVisit: false });
    expect(calls[0]?.method).toBe("GET");
    expect(calls[0]?.url).toBe(`${BASE}${PAGE}/`);
    expect(calls[0]?.query).toEqual({ track_visit: false });
    expect(detail.description_html).toBe("<p>Body</p>");
    expect(detail.issue_ids).toEqual([]);
  });

  it("tracks the visit unless the caller opts out", async () => {
    const { client, calls } = stubClient({ json: () => DETAIL });
    await getPage(client, WS, PROJECT, PAGE);
    expect(calls[0]?.query).toEqual({ track_visit: true });
  });

  it("creates a page with the parent and the initial body", async () => {
    const { client, calls } = stubClient({ json: () => CREATED });
    const created = await createPage(client, WS, PROJECT, {
      name: "Meeting notes",
      access: 1,
      parent: PAGE,
      logo_props: MARK,
      description_html: "<p>Body</p>",
    });
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: BASE,
      json: { name: "Meeting notes", access: 1, parent: PAGE, logo_props: MARK, description_html: "<p>Body</p>" },
    });
    expect(created.id).toBe(PAGE);
    expect(created.issue_ids).toBeUndefined();
  });

  it("patches the record without sending the body", async () => {
    const { client, calls } = stubClient({
      json: () => row({ name: "Renamed", is_locked: true, description_html: "" }),
    });
    const updated = await updatePage(client, WS, PROJECT, PAGE, {
      name: "Renamed",
      access: 1,
      parent: null,
      is_locked: true,
      logo_props: MARK,
    });
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.url).toBe(`${BASE}${PAGE}/`);
    expect(calls[0]?.json).toEqual({ name: "Renamed", access: 1, parent: null, is_locked: true, logo_props: MARK });
    expect(updated.is_locked).toBe(true);
  });

  it("refuses a record patch echo without the record id", async () => {
    const { client } = stubClient({ json: () => ({ name: "Renamed" }) });
    await expect(updatePage(client, WS, PROJECT, PAGE, { name: "Renamed" })).rejects.toThrow();
  });

  it("deletes by id", async () => {
    const { client, calls } = stubClient();
    await deletePage(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "DELETE", url: `${BASE}${PAGE}/` });
  });

  it("duplicates and parses the detached copy", async () => {
    const { client, calls } = stubClient({
      json: () => row({ name: "Meeting notes (Copy)", parent: null, description_html: "<p>Body</p>" }),
    });
    const copy = await duplicatePage(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "POST", url: `${BASE}${PAGE}/duplicate/` });
    expect(copy.name).toBe("Meeting notes (Copy)");
    expect(copy.parent).toBeNull();
  });

  it("surfaces a refused delete", async () => {
    const client = {
      delete: async () => {
        throw new Error("The page should be archived before deleting");
      },
    } as unknown as ApiClient;
    await expect(deletePage(client, WS, PROJECT, PAGE)).rejects.toThrow("archived before deleting");
  });
});

describe("pages state switches", () => {
  it("stars and unstars through the favourite route", async () => {
    const { client, calls } = stubClient();
    await addPageFavorite(client, WS, PROJECT, PAGE);
    await removePageFavorite(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/favorite-pages/${PAGE}/`,
    });
    expect(calls[1]).toMatchObject({
      method: "DELETE",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/favorite-pages/${PAGE}/`,
    });
  });

  it("switches visibility with an access body", async () => {
    const { client, calls } = stubClient();
    await setPageAccess(client, WS, PROJECT, PAGE, 1);
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `${BASE}${PAGE}/access/`,
      json: { access: 1 },
    });
  });

  it("locks and unlocks", async () => {
    const { client, calls } = stubClient();
    await lockPage(client, WS, PROJECT, PAGE);
    await unlockPage(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "POST", url: `${BASE}${PAGE}/lock/` });
    expect(calls[1]).toMatchObject({ method: "DELETE", url: `${BASE}${PAGE}/lock/` });
  });

  it("archives and reads the stamp as the serializer writes it", async () => {
    const { client, calls } = stubClient({ json: () => ({ archived_at: "2026-10-22 12:14:12.607270" }) });
    const result = await archivePage(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "POST", url: `${BASE}${PAGE}/archive/` });
    expect(result.archived_at).toContain("2026-10-22");
  });

  it("rejects an archive answer with no stamp", async () => {
    const { client } = stubClient({ json: () => ({}) });
    await expect(archivePage(client, WS, PROJECT, PAGE)).rejects.toThrow();
  });

  it("restores by deleting the archive stamp", async () => {
    const { client, calls } = stubClient();
    await restorePage(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "DELETE", url: `${BASE}${PAGE}/archive/` });
  });

  it("moves the page to another project", async () => {
    const { client, calls } = stubClient();
    await movePageToProject(client, WS, PROJECT, PAGE, { new_project_id: "623e4567-e89b-12d3-a456-426614174005" });
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `${BASE}${PAGE}/move/`,
      json: { new_project_id: "623e4567-e89b-12d3-a456-426614174005" },
    });
  });

  it("surfaces a 404 from a build without the move route", async () => {
    const client = {
      post: async () => {
        throw new Error("Not found");
      },
    } as unknown as ApiClient;
    await expect(movePageToProject(client, WS, PROJECT, PAGE, { new_project_id: PROJECT })).rejects.toThrow(
      "Not found"
    );
  });
});

describe("pages version and description contracts", () => {
  it("reads an empty version list", async () => {
    const { client, calls } = stubClient({ json: () => [] });
    const versions = await listPageVersions(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "GET", url: `${BASE}${PAGE}/versions/` });
    expect(versions).toEqual([]);
  });

  it("reads one version snapshot with its three stored forms", async () => {
    const { client, calls } = stubClient({
      json: () => ({
        id: VERSION,
        workspace: "523e4567-e89b-12d3-a456-426614174004",
        page: PAGE,
        last_saved_at: "2026-10-20T10:00:00Z",
        description_binary: null,
        description_html: "<p>Old</p>",
        description_json: { type: "doc" },
        owned_by: USER,
        created_at: "2026-10-20T10:00:00Z",
        updated_at: "2026-10-20T10:00:00Z",
        created_by: USER,
        updated_by: USER,
      }),
    });
    const version = await getPageVersion(client, WS, PROJECT, PAGE, VERSION);
    expect(calls[0]?.url).toBe(`${BASE}${PAGE}/versions/${VERSION}/`);
    expect(version.description_html).toBe("<p>Old</p>");
  });

  it("reads the binary body through arrayBuffer", async () => {
    const bytes = new Uint8Array([1, 2, 3, 4]);
    const { client, calls } = stubClient({ raw: () => ({ body: bytes, arrayBuffer: async () => bytes.buffer }) });
    const binary = await getPageDescriptionBinary(client, WS, PROJECT, PAGE);
    expect(calls[0]).toMatchObject({ method: "GET", url: `${BASE}${PAGE}/description/` });
    expect([...binary]).toEqual([1, 2, 3, 4]);
  });

  it("rejects an unreadable body as a contract violation", async () => {
    const { client } = stubClient({
      raw: () => ({ body: "hello", status: 200, text: async () => "hello" }),
    });
    const error: unknown = await getPageDescriptionBinary(client, WS, PROJECT, PAGE).then(
      () => null,
      (rejected: unknown) => rejected
    );
    expect(error).toMatchObject({ code: "contract" });
    expect(error instanceof Error ? error.message : "").toContain("no readable bytes");
  });

  it("patches all three body forms at once", async () => {
    const { client, calls } = stubClient({ json: () => ({ message: "Updated successfully" }) });
    const result = await updatePageDescription(client, WS, PROJECT, PAGE, {
      description_html: "<p>New</p>",
      description_json: { type: "doc" },
      description_binary: "AQID",
    });
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: `${BASE}${PAGE}/description/`,
      json: { description_html: "<p>New</p>", description_json: { type: "doc" }, description_binary: "AQID" },
    });
    expect(result.message).toBe("Updated successfully");
  });

  it("surfaces the locked refusal", async () => {
    const client = {
      sendJson: async () => {
        throw new Error("PAGE_LOCKED");
      },
    } as unknown as ApiClient;
    await expect(updatePageDescription(client, WS, PROJECT, PAGE, { description_html: "<p>x</p>" })).rejects.toThrow(
      "PAGE_LOCKED"
    );
  });
});
