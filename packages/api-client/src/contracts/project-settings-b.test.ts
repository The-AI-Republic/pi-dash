// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub-transport units for the project-settings contracts: URLs, methods,
// request bodies and response parsing for estimate writes, repository
// status/bind/sync/unbind, deploy boards and the generic user-asset upload.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import type { Transport, TransportInit, TransportResponse } from "../client.js";
import { ApiError } from "../errors.js";
import {
  createEstimate,
  deleteEstimate,
  Estimate,
  ESTIMATE_TYPES,
  listProjectEstimates,
  selectActiveEstimate,
} from "./estimates.js";
import {
  bindProjectRepository,
  getProjectRepository,
  ProjectRepositoryBinding,
  setProjectRepositorySync,
  unbindProjectRepository,
} from "./project-repository.js";
import {
  createProjectDeployBoard,
  deleteProjectDeployBoard,
  getProjectDeployBoard,
  ProjectDeployBoard,
  updateProjectDeployBoard,
} from "./project-deploy-boards.js";
import { uploadUserAsset, uploadUserAvatar } from "./assets.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const AT = "2026-10-16T15:32:05.479661+09:00";

interface Call {
  url: string;
  method: string;
  body: string | undefined;
}

function response(body: unknown, status = 200): TransportResponse {
  return {
    status,
    url: "https://api.test/x",
    headers: { get: () => "application/json" },
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  };
}

function recording(handler: (call: Call) => TransportResponse): Transport & { calls: Call[] } {
  const calls: Call[] = [];
  const transport = (async (url: string, init: TransportInit) => {
    const call = { url, method: init.method, body: init.body };
    calls.push(call);
    return handler(call);
  }) as unknown as Transport & { calls: Call[] };
  transport.calls = calls;
  return transport;
}

function client(transport: Transport) {
  return createClient({ baseUrl: "https://api.test", transport, csrfToken: async () => "csrf" });
}

const ESTIMATE_ROW = {
  id: UUID,
  workspace: UUID2,
  project: UUID,
  name: "Sizes",
  description: "",
  type: "categories",
  last_used: true,
  points: [{ id: UUID2, key: 1, value: "S", description: "" }],
  created_at: AT,
  updated_at: AT,
  created_by: null,
  updated_by: null,
};

describe("estimate contracts", () => {
  it("parses the list read and selects the active system", async () => {
    const transport = recording(() => response([ESTIMATE_ROW]));
    const rows = await listProjectEstimates(client(transport), "acme", UUID);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/estimates/`);
    expect(selectActiveEstimate(rows)?.name).toBe("Sizes");
    expect(Estimate.parse(rows[0])).toEqual(rows[0]);
  });

  it("posts the estimate and point rows and parses the created system", async () => {
    const transport = recording(() => response({ ...ESTIMATE_ROW, last_used: false }));
    const created = await createEstimate(client(transport), "acme", UUID, {
      name: "Sizes",
      type: "points",
      last_used: false,
      points: [{ key: 1, value: "S" }],
    });
    const call = transport.calls[0];
    expect(call?.method).toBe("POST");
    expect(call?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/estimates/`);
    expect(JSON.parse(call?.body ?? "{}")).toEqual({
      estimate: { name: "Sizes", type: "points", last_used: false },
      estimate_points: [{ key: 1, value: "S" }],
    });
    expect(created.last_used).toBe(false);
  });

  it("sends only the name when the caller omits the optional keys", async () => {
    const transport = recording(() => response(ESTIMATE_ROW));
    await createEstimate(client(transport), "acme", UUID, { name: "Bare" });
    expect(JSON.parse(transport.calls[0]?.body ?? "{}")).toEqual({
      estimate: { name: "Bare" },
      estimate_points: [],
    });
  });

  it("deletes by id and resolves the status", async () => {
    const transport = recording(() => response(undefined, 204));
    await expect(deleteEstimate(client(transport), "acme", UUID, UUID2)).resolves.toBe(204);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/estimates/${UUID2}/`);
    expect(transport.calls[0]?.method).toBe("DELETE");
  });

  it("names the system types the server accepts", () => {
    expect(ESTIMATE_TYPES).toEqual(["categories", "points", "time"]);
  });
});

const BINDING_ROW = {
  bound: true,
  id: UUID,
  provider: "github",
  provider_account_id: UUID2,
  host_url: "https://github.com",
  repository: {
    id: "42",
    provider: "github",
    host_url: "https://github.com",
    namespace: "acme",
    name: "widgets",
    full_name: "acme/widgets",
    web_url: "https://github.com/acme/widgets",
    clone_url_http: "https://github.com/acme/widgets.git",
    clone_url_ssh: "git@github.com:acme/widgets.git",
    default_branch: "main",
    private: false,
  },
  is_sync_enabled: true,
  clone_auth_mode: "public",
  last_synced_at: AT,
  last_sync_error: "",
  degraded: false,
  degraded_reason: null,
};

describe("repository contracts", () => {
  it("parses the unbound marker", async () => {
    const transport = recording(() => response({ bound: false }));
    await expect(getProjectRepository(client(transport), "acme", UUID)).resolves.toEqual({ bound: false });
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/repository/`);
  });

  it("parses a bound row", async () => {
    const transport = recording(() => response(BINDING_ROW));
    const status = await getProjectRepository(client(transport), "acme", UUID);
    expect(status).toEqual(ProjectRepositoryBinding.parse(BINDING_ROW));
  });

  it("posts the bind input and parses the canonical repo URL", async () => {
    const transport = recording(() =>
      response({ ...BINDING_ROW, repo_url: "https://github.com/acme/widgets.git" }, 201)
    );
    const bound = await bindProjectRepository(client(transport), "acme", UUID, {
      repo_url: "https://github.com/acme/widgets",
      provider_account_id: UUID2,
    });
    expect(JSON.parse(transport.calls[0]?.body ?? "{}")).toEqual({
      repo_url: "https://github.com/acme/widgets",
      provider_account_id: UUID2,
    });
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/repository/bind/`);
    expect(bound.repo_url).toContain("widgets");
  });

  it("patches the sync switch with a bare boolean", async () => {
    const transport = recording(() => response({ ...BINDING_ROW, is_sync_enabled: false }));
    const updated = await setProjectRepositorySync(client(transport), "acme", UUID, false);
    expect(transport.calls[0]?.method).toBe("PATCH");
    expect(JSON.parse(transport.calls[0]?.body ?? "{}")).toEqual({ enabled: false });
    expect(updated.is_sync_enabled).toBe(false);
  });

  it("deletes the binding and parses the unbound marker", async () => {
    const transport = recording(() => response({ bound: false }));
    await expect(unbindProjectRepository(client(transport), "acme", UUID)).resolves.toEqual({ bound: false });
    expect(transport.calls[0]?.method).toBe("DELETE");
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/repository/`);
  });
});

const BOARD_ROW = {
  id: UUID,
  project_details: {
    id: UUID2,
    identifier: "CT",
    name: "Contract",
    cover_image: null,
    cover_image_url: null,
    logo_props: {},
    description: "",
    is_default: true,
  },
  workspace_detail: { name: "Acme", slug: "acme", id: UUID, logo_url: null },
  created_at: AT,
  updated_at: AT,
  created_by: null,
  updated_by: null,
  deleted_at: null,
  workspace: UUID,
  project: UUID2,
  entity_identifier: UUID2,
  entity_name: "project",
  anchor: "abc123",
  is_comments_enabled: true,
  is_reactions_enabled: false,
  intake: null,
  is_votes_enabled: false,
  view_props: { list: true, kanban: true },
  is_activity_enabled: true,
  is_disabled: false,
};

const BOARD_EMPTY = {
  deleted_at: null,
  entity_identifier: null,
  entity_name: "",
  is_comments_enabled: false,
  is_reactions_enabled: false,
  is_votes_enabled: false,
  view_props: null,
  is_activity_enabled: false,
  is_disabled: false,
  created_by: null,
  updated_by: null,
  intake: null,
};

describe("deploy board contracts", () => {
  it("parses a published board and the empty shell", async () => {
    const published = recording(() => response(BOARD_ROW));
    await expect(getProjectDeployBoard(client(published), "acme", UUID2)).resolves.toMatchObject({
      id: UUID,
      anchor: "abc123",
    });
    const empty = recording(() => response(BOARD_EMPTY));
    const shell = await getProjectDeployBoard(client(empty), "acme", UUID2);
    expect(shell).not.toHaveProperty("anchor");
    expect(shell.entity_name).toBe("");
  });

  it("posts the layout map under views with the flags", async () => {
    const transport = recording(() => response(BOARD_ROW));
    await createProjectDeployBoard(client(transport), "acme", UUID2, {
      views: { list: true, kanban: true },
      is_comments_enabled: true,
      intake: UUID,
    });
    expect(transport.calls[0]?.method).toBe("POST");
    expect(JSON.parse(transport.calls[0]?.body ?? "{}")).toEqual({
      views: { list: true, kanban: true },
      is_comments_enabled: true,
      intake: UUID,
    });
    expect(transport.calls[0]?.url).toBe(
      `https://api.test/api/workspaces/acme/projects/${UUID2}/project-deploy-boards/`
    );
  });

  it("patches the board and parses the row", async () => {
    const transport = recording(() => response({ ...BOARD_ROW, view_props: { list: true } }));
    const updated = await updateProjectDeployBoard(client(transport), "acme", UUID2, UUID, {
      view_props: { list: true },
      is_votes_enabled: true,
    });
    expect(transport.calls[0]?.method).toBe("PATCH");
    expect(transport.calls[0]?.url).toBe(
      `https://api.test/api/workspaces/acme/projects/${UUID2}/project-deploy-boards/${UUID}/`
    );
    expect(updated).toEqual(ProjectDeployBoard.parse({ ...BOARD_ROW, view_props: { list: true } }));
  });

  it("deletes the board and resolves the status", async () => {
    const transport = recording(() => response(undefined, 204));
    await expect(deleteProjectDeployBoard(client(transport), "acme", UUID2, UUID)).resolves.toBe(204);
    expect(transport.calls[0]?.method).toBe("DELETE");
  });
});

describe("generic user-asset upload", () => {
  const grant = {
    asset_id: "asset-9",
    asset_url: "/api/assets/v2/static/asset-9/",
    upload_data: { url: "https://uploads.test/signed", fields: { key: "k", policy: "p" } },
  };

  function grantTransport(): Transport & { calls: Call[] } {
    return recording((call) => {
      if (call.method === "POST") return response(grant, 201);
      return response(undefined, 204);
    });
  }

  it("grants the chosen entity type, posts bytes and confirms", async () => {
    const transport = grantTransport();
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });
    const uploads: Array<{ url: string; body: FormData }> = [];
    const url = await uploadUserAsset(client(transport), "USER_COVER", file, async (target, body) => {
      uploads.push({ url: target, body });
      return { ok: true, status: 204 };
    });
    expect(url).toBe("/api/assets/v2/static/asset-9/");
    const grantCall = transport.calls.find((call) => call.method === "POST");
    expect(grantCall?.url).toBe("https://api.test/api/assets/v2/user-assets/");
    expect(JSON.parse(grantCall?.body ?? "{}")).toMatchObject({ entity_type: "USER_COVER", name: "emblem.png" });
    expect(uploads[0]?.url).toBe("https://uploads.test/signed");
    expect(uploads[0]?.body.get("file")).toBe(file);
    expect(transport.calls.find((call) => call.method === "PATCH")?.url).toBe(
      "https://api.test/api/assets/v2/user-assets/asset-9/"
    );
  });

  it("rejects a refused byte leg with its status", async () => {
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });
    const failure = await uploadUserAsset(client(grantTransport()), "USER_AVATAR", file, async () => ({
      ok: false,
      status: 403,
    })).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(403);
  });

  it("keeps the avatar wrapper on the avatar entity type", async () => {
    const transport = grantTransport();
    const file = new File(["bytes"], "ada.png", { type: "image/png" });
    await uploadUserAvatar(client(transport), file, async () => ({ ok: true, status: 204 }));
    const grantCall = transport.calls.find((call) => call.method === "POST");
    expect(JSON.parse(grantCall?.body ?? "{}")).toMatchObject({ entity_type: "USER_AVATAR" });
  });
});
