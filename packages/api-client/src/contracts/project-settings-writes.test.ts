// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings member and state writer contracts against a stub
// transport. Live-server proof lives in members-states.contract.test.ts.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { createClient } from "../client.js";
import type { ApiClient } from "../client.js";
import {
  addProjectMembers,
  createState,
  deleteState,
  markStateDefault,
  removeProjectMember,
  updateProjectMember,
  updateState,
} from "./project-settings-writes.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";
const UUID4 = "423e4567-e89b-12d3-a456-426614174003";
const AT = "2026-10-16T15:32:05.479661+09:00";

function stubClient(impl: { sendJson?: ApiClient["sendJson"]; delete?: ApiClient["delete"] }): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown, what: string) => {
      try {
        return schema.parse(value);
      } catch (error) {
        throw new ApiError({
          status: 0,
          code: "contract",
          message: `${what} did not match its contract`,
          cause: error,
        });
      }
    }) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

const MEMBER_ROLE_ROW = {
  id: UUID,
  role: 15,
  member: UUID2,
  project: UUID3,
  original_role: 5,
  created_at: AT,
};

const MEMBER_ADMIN_ROW = {
  id: UUID,
  member: {
    id: UUID2,
    first_name: "Ada",
    last_name: "Lovelace",
    avatar: "",
    avatar_url: null,
    display_name: "Ada",
    is_bot: false,
    email: "ada@example.com",
    last_login_medium: "email",
  },
  workspace: { id: UUID3, name: "Acme", slug: "acme", logo_url: null },
  project: {
    id: UUID4,
    identifier: "CT",
    name: "Contract",
    cover_image: null,
    cover_image_url: null,
    logo_props: {},
    description: "",
    is_default: true,
  },
  role: 15,
  comment: null,
  view_props: {},
  default_props: {},
  preferences: {},
  sort_order: 65535,
  is_active: true,
  created_at: AT,
  updated_at: AT,
  created_by: null,
  updated_by: null,
  deleted_at: null,
};

const STATE_ROW = {
  id: UUID,
  project_id: UUID2,
  workspace_id: UUID3,
  name: "Blocked",
  color: "#ff0000",
  group: "started",
  default: false,
  description: "Waiting",
  sequence: 12000,
  order: 0.5,
};

describe("addProjectMembers", () => {
  it("posts the members array and parses every returned role row", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: [MEMBER_ROLE_ROW], status: 201, url };
      }) as ApiClient["sendJson"],
    });
    const members = [{ member_id: UUID2, role: 15 }];
    const rows = await addProjectMembers(client, "acme", UUID3, members);
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/acme/projects/${UUID3}/members/`,
      json: { members },
    });
    expect(rows[0]?.member).toBe(UUID2);
    expect(rows[0]?.original_role).toBe(5);
  });

  it("rejects a response that does not match the role projection", async () => {
    const client = stubClient({
      sendJson: (async (_method: string, url: string) => ({
        data: [{ ...MEMBER_ROLE_ROW, id: "not-a-uuid" }],
        status: 201,
        url,
      })) as ApiClient["sendJson"],
    });
    await expect(addProjectMembers(client, "acme", UUID3, [{ member_id: UUID2, role: 15 }])).rejects.toBeInstanceOf(
      ApiError
    );
  });
});

describe("updateProjectMember", () => {
  it("patches only the role and parses the nested admin row", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: MEMBER_ADMIN_ROW, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const row = await updateProjectMember(client, "acme", UUID3, UUID, 15);
    expect(calls[0]).toEqual({
      method: "PATCH",
      url: `/api/workspaces/acme/projects/${UUID3}/members/${UUID}/`,
      json: { role: 15 },
    });
    expect(row.member.display_name).toBe("Ada");
    expect(row.workspace.slug).toBe("acme");
    expect(row.project.identifier).toBe("CT");
  });
});

describe("removeProjectMember", () => {
  it("deletes the membership and resolves the status", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }) as ApiClient["delete"],
    });
    await expect(removeProjectMember(client, "acme", UUID3, UUID)).resolves.toBe(204);
    expect(calls).toEqual([`/api/workspaces/acme/projects/${UUID3}/members/${UUID}/`]);
  });
});

describe("createState", () => {
  it("posts the form fields and parses the state", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: STATE_ROW, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const state = await createState(client, "acme", UUID2, {
      name: "Blocked",
      color: "#ff0000",
      group: "started",
      description: "Waiting",
    });
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/acme/projects/${UUID2}/states/`,
      json: { name: "Blocked", color: "#ff0000", group: "started", description: "Waiting" },
    });
    expect(state.group).toBe("started");
  });
});

describe("updateState", () => {
  it("patches rename, group and sequence", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: { ...STATE_ROW, sequence: 12000, order: 0.25 }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const state = await updateState(client, "acme", UUID2, UUID, {
      name: "Blocked",
      group: "review",
      sequence: 12000,
    });
    expect(calls[0]).toEqual({
      method: "PATCH",
      url: `/api/workspaces/acme/projects/${UUID2}/states/${UUID}/`,
      json: { name: "Blocked", group: "review", sequence: 12000 },
    });
    expect(state.sequence).toBe(12000);
    expect(state.order).toBe(0.25);
  });
});

describe("deleteState", () => {
  it("deletes the state and resolves the status", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }) as ApiClient["delete"],
    });
    await expect(deleteState(client, "acme", UUID2, UUID)).resolves.toBe(204);
    expect(calls).toEqual([`/api/workspaces/acme/projects/${UUID2}/states/${UUID}/`]);
  });
});

describe("markStateDefault", () => {
  it("posts an empty body to mark-default and resolves the status", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: null, status: 204, url };
      }) as ApiClient["sendJson"],
    });
    await expect(markStateDefault(client, "acme", UUID2, UUID)).resolves.toBe(204);
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/acme/projects/${UUID2}/states/${UUID}/mark-default/`,
      json: {},
    });
  });
});

describe("contract client with a recording transport", () => {
  it("builds the members route and serializes the JSON payload", async () => {
    const urls: string[] = [];
    const bodies: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) {
          return {
            status: 200,
            url,
            headers: { get: (name) => (name.toLowerCase() === "content-type" ? "application/json" : null) },
            text: async () => JSON.stringify({ csrf_token: "test-token" }),
          };
        }
        urls.push(url);
        bodies.push(init.body ?? "");
        return {
          status: 201,
          url,
          headers: { get: (name) => (name.toLowerCase() === "content-type" ? "application/json" : null) },
          text: async () => JSON.stringify([MEMBER_ROLE_ROW]),
        };
      },
    });
    await addProjectMembers(client, "acme", UUID3, [{ member_id: UUID2, role: 15 }]);
    expect(urls[0]).toBe(`https://api.test/api/workspaces/acme/projects/${UUID3}/members/`);
    expect(JSON.parse(bodies[0] ?? "{}")).toEqual({ members: [{ member_id: UUID2, role: 15 }] });
  });
});
