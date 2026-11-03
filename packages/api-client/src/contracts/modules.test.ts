// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-module contracts against a stub client (no backend), followed by
// the modules area's own surface (row/detail/link schemas, path builders, the
// full CRUD surface and its refusal envelopes).
import { describe, expect, it } from "vitest";
import { createClient, type ApiClient, type Transport, type TransportInit } from "../client.js";
import { ApiError } from "../errors.js";
import {
  ARCHIVABLE_MODULE_STATUSES,
  ModuleArchiveError,
  ModuleArchiveStamp,
  ModuleCompletionChart,
  ModuleDetail,
  ModuleDistribution,
  ModuleErrorBody,
  ModuleEstimateDistribution,
  ModuleFavoriteInput,
  ModuleLink,
  ModuleLinkList,
  ModuleList,
  ModuleListItem,
  ModuleMembershipInput,
  ModuleRow,
  ModuleRowList,
  ModuleUpdate,
  ModuleWriteEcho,
  archivedModulePath,
  archivedModulesPath,
  archiveModule,
  createModule,
  createModuleLink,
  deleteModule,
  deleteModuleLink,
  favoriteModule,
  getArchivedModule,
  getModule,
  listArchivedModules,
  listModuleLinks,
  listModules,
  listProjectModules,
  listWorkspaceModules,
  moduleArchivePath,
  moduleDetailPath,
  moduleFavoritePath,
  moduleFavoritesPath,
  moduleIssuePath,
  moduleIssuesPath,
  moduleLinkPath,
  moduleLinksPath,
  modulesCollection,
  removeIssueFromModule,
  replaceModule,
  restoreModule,
  unfavoriteModule,
  updateModule,
  updateModuleLink,
  workspaceModulesPath,
} from "./modules.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

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

const ARCHIVED_ROW = {
  id: UUID,
  workspace_id: UUID2,
  project_id: UUID2,
  name: "Archived module",
  description: "",
  description_text: null,
  description_html: null,
  start_date: "2026-09-01",
  target_date: "2026-09-30",
  status: "completed",
  lead_id: UUID2,
  member_ids: [UUID2],
  sort_order: 1024,
  total_issues: 2,
  is_favorite: false,
  cancelled_issues: 0,
  completed_issues: 2,
  started_issues: 0,
  unstarted_issues: 0,
  backlog_issues: 0,
  created_at: "2026-09-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
  archived_at: "2026-10-01T00:00:00Z",
};

describe("listArchivedModules", () => {
  it("reads the flat archived list and parses its rows", async () => {
    const { client, urls } = stubClient([ARCHIVED_ROW]);
    const rows = await listArchivedModules(client, "acme", UUID2);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-modules/`]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.status).toBe("completed");
    expect(rows[0]?.member_ids).toEqual([UUID2]);
  });

  it("rejects a row without the manual placement the archived screen sorts by", async () => {
    const { sort_order: _dropped, ...rest } = ARCHIVED_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listArchivedModules(client, "acme", UUID2)).rejects.toThrow();
  });

  it("reads the manual placement a row carries", async () => {
    const { client } = stubClient([ARCHIVED_ROW]);
    const rows = await listArchivedModules(client, "acme", UUID2);
    expect(rows[0]?.sort_order).toBe(1024);
  });

  it("rejects a row without the progress counts", async () => {
    const { total_issues: _dropped, ...rest } = ARCHIVED_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listArchivedModules(client, "acme", UUID2)).rejects.toThrow();
  });
});

const ARCHIVED_DETAIL = {
  ...ARCHIVED_ROW,
  link_module: [{ id: UUID2, title: "Spec", url: "https://example.test/spec" }],
  sub_issues: 0,
  total_estimate_points: 3,
  completed_estimate_points: 3,
  backlog_estimate_points: 0,
  unstarted_estimate_points: 0,
  started_estimate_points: 0,
  cancelled_estimate_points: 0,
  estimate_distribution: {},
  distribution: { assignees: [], labels: [], completion_chart: {} },
};

describe("getArchivedModule", () => {
  it("reads one archived module with its links and estimate sums", async () => {
    const { client, urls } = stubClient(ARCHIVED_DETAIL);
    const detail = await getArchivedModule(client, "acme", UUID2, UUID);
    expect(urls).toEqual([`/api/workspaces/acme/projects/${UUID2}/archived-modules/${UUID}/`]);
    expect(detail.name).toBe("Archived module");
    expect(detail.link_module).toHaveLength(1);
    expect(detail.completed_estimate_points).toBe(3);
  });

  it("rejects a detail without the distributions", async () => {
    const { distribution: _dropped, ...rest } = ARCHIVED_DETAIL;
    void _dropped;
    const { client } = stubClient(rest);
    await expect(getArchivedModule(client, "acme", UUID2, UUID)).rejects.toThrow();
  });
});

function writeClient(data: unknown, status: number): { client: ApiClient; calls: { method: string; url: string }[] } {
  const calls: { method: string; url: string }[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    sendJson: (async (method: string, url: string) => {
      calls.push({ method, url });
      return { data, status, url };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.push({ method: "DELETE", url });
      return { status, url };
    }) as ApiClient["delete"],
  } as ApiClient;
  return { client, calls };
}

describe("archiveModule", () => {
  it("posts to the module archive route and parses the stamp", async () => {
    const { client, calls } = writeClient({ archived_at: "2026-10-21 12:00:00+00:00" }, 200);
    const stamp = await archiveModule(client, "acme", UUID2, UUID);
    expect(calls).toEqual([{ method: "POST", url: `/api/workspaces/acme/projects/${UUID2}/modules/${UUID}/archive/` }]);
    expect(stamp.archived_at).toBe("2026-10-21 12:00:00+00:00");
  });
});

describe("restoreModule", () => {
  it("deletes the module archive route and resolves the status", async () => {
    const { client, calls } = writeClient(null, 204);
    const status = await restoreModule(client, "acme", UUID2, UUID);
    expect(calls).toEqual([
      { method: "DELETE", url: `/api/workspaces/acme/projects/${UUID2}/modules/${UUID}/archive/` },
    ]);
    expect(status).toBe(204);
  });
});

describe("listWorkspaceModules", () => {
  const WORKSPACE_ROW = {
    id: UUID,
    workspace_id: UUID2,
    project_id: UUID2,
    name: "Alpha",
    status: "planned",
    total_issues: 2,
    completed_issues: 1,
  };

  it("reads the workspace module list without a project segment", async () => {
    const { client, urls } = stubClient([WORKSPACE_ROW]);
    const rows = await listWorkspaceModules(client, "acme");
    expect(urls).toEqual(["/api/workspaces/acme/modules/"]);
    expect(rows.map((row) => row.name)).toEqual(["Alpha"]);
  });

  it("accepts the favorite flag when the projection carries it", async () => {
    const { client } = stubClient([{ ...WORKSPACE_ROW, is_favorite: true }]);
    const rows = await listWorkspaceModules(client, "acme");
    expect(rows[0]?.is_favorite).toBe(true);
  });

  it("rejects a row without its identity", async () => {
    const { name: _dropped, ...rest } = WORKSPACE_ROW;
    void _dropped;
    const { client } = stubClient([rest]);
    await expect(listWorkspaceModules(client, "acme")).rejects.toThrow();
  });
});

const SLUG = "acme";
const PROJECT = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MODULE = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const MODULE_2 = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const WORKSPACE = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const USER = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const LINK = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const ISSUE = "11111111-1111-4111-8111-111111111111";
const LABEL = "22222222-2222-4222-8222-222222222222";

const ROW = {
  id: MODULE,
  workspace_id: WORKSPACE,
  project_id: PROJECT,
  name: "Checkout revamp",
  description: "Rework the checkout flow",
  description_text: { type: "doc" },
  description_html: { type: "doc" },
  start_date: "2026-09-01",
  target_date: "2026-10-31",
  status: "in-progress",
  lead_id: USER,
  member_ids: [USER],
  view_props: { layout: "list" },
  logo_props: {},
  sort_order: 1024,
  external_source: null,
  external_id: null,
  is_favorite: true,
  total_issues: 12,
  completed_issues: 4,
  cancelled_issues: 1,
  started_issues: 3,
  unstarted_issues: 2,
  backlog_issues: 2,
  total_estimate_points: 34,
  completed_estimate_points: 11,
  backlog_estimate_points: 6,
  unstarted_estimate_points: 5,
  started_estimate_points: 9,
  cancelled_estimate_points: 3,
  created_at: "2026-08-20T10:00:00Z",
  updated_at: "2026-10-16T15:32:05.479661+09:00",
  archived_at: null,
  created_by: USER,
  updated_by: null,
} as const;

const LINK_ROW = {
  id: LINK,
  project: PROJECT,
  workspace: WORKSPACE,
  module: MODULE,
  title: "Design doc",
  url: "https://example.test/design",
  metadata: {},
  created_at: "2026-09-02T09:00:00Z",
  updated_at: "2026-09-02T09:00:00Z",
  created_by: USER,
  updated_by: null,
  deleted_at: null,
} as const;

const DISTRIBUTION = {
  assignees: [
    {
      assignee_id: USER,
      avatar_url: null,
      first_name: "Ada",
      last_name: "Lovelace",
      display_name: "Ada Lovelace",
      total_issues: 6,
      completed_issues: 2,
      pending_issues: 4,
    },
    {
      assignee_id: null,
      avatar_url: null,
      first_name: null,
      last_name: null,
      display_name: null,
      total_issues: 0,
      completed_issues: 0,
      pending_issues: 0,
    },
  ],
  labels: [
    {
      label_id: LABEL,
      label_name: "checkout",
      color: "#ff8800",
      total_issues: 5,
      completed_issues: 1,
      pending_issues: 4,
    },
  ],
  completion_chart: { "2026-09-01": 12, "2026-09-02": 10, "2026-12-01": null },
} as const;

const DETAIL = {
  ...ROW,
  link_module: [LINK_ROW],
  sub_issues: 2,
  distribution: DISTRIBUTION,
  estimate_distribution: {},
} as const;

/**
 * What PUT really answers: the write serializer's representation, with
 * relations under their model names and none of the read annotations.
 */
const PUT_ECHO = {
  id: MODULE,
  workspace: WORKSPACE,
  project: PROJECT,
  name: "Checkout revamp",
  description: "Rework the checkout flow",
  description_text: null,
  description_html: null,
  start_date: "2026-09-01",
  target_date: "2026-10-31",
  status: "in-progress",
  lead: USER,
  lead_id: USER,
  members: [USER],
  member_ids: [USER],
  view_props: { layout: "list" },
  logo_props: {},
  sort_order: 1024,
  external_source: null,
  external_id: null,
  created_at: "2026-08-20T10:00:00Z",
  updated_at: "2026-10-16T15:32:05.479661+09:00",
  created_by: USER,
  updated_by: USER,
  deleted_at: null,
  archived_at: null,
} as const;

interface Call {
  url: string;
  init: TransportInit;
}

/** Real client over a recording transport; every response carries `body`. */
function client(body: unknown, status = 200): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const transport = (async (url: string, init: TransportInit) => {
    calls.push({ url, init });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    return {
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => (body === undefined ? "" : JSON.stringify(body)),
    };
  }) as Transport;
  return { client: createClient({ baseUrl: "https://api.test", transport }), calls };
}

/** Recorded calls minus the client's CSRF bootstrap, with the base stripped. */
function requests(calls: Call[]): Call[] {
  return calls
    .filter((call) => !call.url.endsWith("/auth/get-csrf-token/"))
    .map((call) => ({ ...call, url: call.url.replace("https://api.test", "") }));
}

describe("module row schemas", () => {
  it("parses a full row and the collections that carry it", () => {
    const row = ModuleRow.parse(ROW);
    expect(row.id).toBe(MODULE);
    expect(row.status).toBe("in-progress");
    expect(row.is_favorite).toBe(true);
    expect(row.total_issues).toBe(12);
    expect(ModuleRowList.parse([ROW])).toHaveLength(1);
    expect(
      ModuleListItem.parse({ id: MODULE, project_id: PROJECT, name: "M", status: "planned", is_favorite: false })
    ).toBeTruthy();
    expect(
      ModuleList.parse([{ id: MODULE, project_id: PROJECT, name: "M", status: "planned", is_favorite: false }])
    ).toHaveLength(1);
  });

  it("keeps the update echo's calendar dates and accepts a full replace row", () => {
    const update = ModuleUpdate.parse({
      id: MODULE,
      name: "Checkout revamp",
      project_id: PROJECT,
      description: "",
      start_date: "2026-09-01",
      target_date: null,
      status: "planned",
    });
    expect(update.start_date).toBe("2026-09-01");
    expect(update.target_date).toBeNull();
  });

  it("rejects a row with a missing field, an unknown status or a bad date", () => {
    const { name: _name, ...withoutName } = ROW;
    expect(() => ModuleRow.parse(withoutName)).toThrow();
    expect(() => ModuleRow.parse({ ...ROW, status: "done" })).toThrow();
    expect(() => ModuleRow.parse({ ...ROW, start_date: "01/09/2026" })).toThrow();
    expect(() => ModuleRow.parse({ ...ROW, created_at: "yesterday" })).toThrow();
  });

  it("rejects an update echo without a name or with a datetime date", () => {
    expect(() =>
      ModuleUpdate.parse({ id: MODULE, project_id: PROJECT, start_date: null, target_date: null })
    ).toThrow();
    expect(() =>
      ModuleUpdate.parse({
        id: MODULE,
        name: "M",
        project_id: PROJECT,
        start_date: "2026-09-01T00:00:00Z",
        target_date: null,
      })
    ).toThrow();
  });
});

describe("module detail schemas", () => {
  it("parses the detail with its distribution and links", () => {
    const detail = ModuleDetail.parse(DETAIL);
    expect(detail.sub_issues).toBe(2);
    expect(detail.link_module).toHaveLength(1);
    expect(detail.distribution.assignees[0]?.display_name).toBe("Ada Lovelace");
    expect(detail.distribution.completion_chart["2026-12-01"]).toBeNull();
    expect(detail.estimate_distribution).toEqual({});
    expect(ModuleDetail.parse(DETAIL).id).toBe(MODULE);
  });

  it("parses an estimate distribution when the project estimates in points", () => {
    const payload = {
      ...DETAIL,
      estimate_distribution: {
        assignees: [
          {
            assignee_id: USER,
            avatar_url: "https://example.test/a.png",
            first_name: "Ada",
            last_name: "Lovelace",
            display_name: "Ada Lovelace",
            total_estimates: 8,
            completed_estimates: null,
            pending_estimates: 8,
          },
        ],
        labels: [
          {
            label_id: LABEL,
            label_name: "checkout",
            color: "#ff8800",
            total_estimates: 8,
            completed_estimates: 2,
            pending_estimates: 6,
          },
        ],
        completion_chart: { "2026-09-01": 8 },
      },
    };
    const detail = ModuleDetail.parse(payload);
    expect(detail.estimate_distribution.assignees?.[0]?.total_estimates).toBe(8);
    expect(detail.estimate_distribution.completion_chart?.["2026-09-01"]).toBe(8);
  });

  it("rejects a detail whose distribution or counts are malformed", () => {
    expect(() => ModuleDetail.parse({ ...DETAIL, distribution: { assignees: [] } })).toThrow();
    expect(() =>
      ModuleDetail.parse({
        ...DETAIL,
        distribution: { ...DISTRIBUTION, completion_chart: { "2026-09-01": "many" } },
      })
    ).toThrow();
    expect(() => ModuleDetail.parse({ ...DETAIL, link_module: [{ id: LINK, url: 7, title: null }] })).toThrow();
    expect(() => ModuleDetail.parse({ ...DETAIL, sub_issues: "2" })).toThrow();
  });

  it("parses the distribution and chart schemas on their own", () => {
    expect(ModuleDistribution.parse(DISTRIBUTION).labels).toHaveLength(1);
    expect(ModuleEstimateDistribution.parse({})).toEqual({});
    expect(ModuleCompletionChart.parse({ "2026-09-01": 3.5 })).toEqual({ "2026-09-01": 3.5 });
  });
});

describe("ModuleWriteEcho", () => {
  it("parses the write serializer's answer with cleared optionals", () => {
    const echo = ModuleWriteEcho.parse({ ...PUT_ECHO, workspace: WORKSPACE, created_by: null, deleted_at: null });
    expect(echo.workspace).toBe(WORKSPACE);
    expect(echo.created_by).toBeNull();
    expect(echo.members).toEqual([USER]);
  });

  it("rejects a read row, a missing name and a bad audit stamp", () => {
    expect(() => ModuleWriteEcho.parse(ROW)).toThrow();
    const { name: _name, ...nameless } = PUT_ECHO;
    void _name;
    expect(() => ModuleWriteEcho.parse(nameless)).toThrow();
    expect(() => ModuleWriteEcho.parse({ ...PUT_ECHO, created_at: "yesterday" })).toThrow();
    expect(() => ModuleWriteEcho.parse({ ...PUT_ECHO, project: "not-a-uuid" })).toThrow();
  });
});

describe("module link schemas", () => {
  it("parses links and the link list", () => {
    expect(ModuleLink.parse(LINK_ROW).url).toBe("https://example.test/design");
    expect(ModuleLinkList.parse([LINK_ROW])).toHaveLength(1);
    expect(ModuleLink.parse({ id: LINK, url: "http://example.test", title: null })).toBeTruthy();
  });

  it("rejects links without an id or a url", () => {
    expect(() => ModuleLink.parse({ id: LINK, title: "No url" })).toThrow();
    expect(() => ModuleLink.parse({ url: "https://example.test", title: null })).toThrow();
    expect(() =>
      ModuleLink.parse({ id: LINK, url: "https://example.test", title: null, created_at: "today" })
    ).toThrow();
  });
});

describe("archive and error schemas", () => {
  it("parses the archive stamp and names the archivable statuses", () => {
    expect(ModuleArchiveStamp.parse({ archived_at: "2026-10-20 11:00:00.123456+00:00" }).archived_at).toContain("2026");
    expect(ARCHIVABLE_MODULE_STATUSES).toEqual(["completed", "cancelled"]);
  });

  it("rejects an archive stamp that is not an object with a string", () => {
    expect(() => ModuleArchiveStamp.parse({})).toThrow();
    expect(() => ModuleArchiveStamp.parse({ archived_at: 42 })).toThrow();
    expect(() => ModuleArchiveError.parse({ message: "nope" })).toThrow();
    expect(ModuleArchiveError.parse({ error: "Only completed or cancelled modules can be archived" }).error).toContain(
      "completed"
    );
  });

  it("parses both refusal envelopes and rejects an empty body", () => {
    expect(ModuleErrorBody.parse({ error: "Archived module cannot be updated" })).toBeTruthy();
    expect(ModuleErrorBody.parse({ error: ["Module with this name already exists"] })).toBeTruthy();
    expect(ModuleErrorBody.parse({ name: ["This field may not be blank."] })).toBeTruthy();
    expect(() => ModuleErrorBody.parse({})).toThrow();
    expect(() => ModuleErrorBody.parse({ name: "not a list" })).toThrow();
  });

  it("parses the work-item membership body and rejects a non-uuid module", () => {
    const both = ModuleMembershipInput.parse({ modules: [UUID], removed_modules: [UUID2] });
    expect(both.modules).toEqual([UUID]);
    expect(ModuleMembershipInput.parse({ modules: [] })).toEqual({ modules: [] });
    expect(() => ModuleMembershipInput.parse({ modules: ["not-a-uuid"] })).toThrow();
    expect(() => ModuleMembershipInput.parse({ removed_modules: [UUID] })).toThrow();
  });

  it("names the module in the favorites write", () => {
    expect(ModuleFavoriteInput.parse({ module: MODULE }).module).toBe(MODULE);
    expect(() => ModuleFavoriteInput.parse({ module: "not-a-uuid" })).toThrow();
  });
});

describe("module path builders", () => {
  it("builds the collection, detail, work-item, link, favorite and archive paths", () => {
    expect(modulesCollection(SLUG, PROJECT)).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/`);
    expect(moduleDetailPath(SLUG, PROJECT, MODULE)).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/`);
    expect(moduleIssuesPath(SLUG, PROJECT, MODULE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/issues/`
    );
    expect(moduleIssuePath(SLUG, PROJECT, MODULE, ISSUE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/issues/${ISSUE}/`
    );
    expect(moduleLinksPath(SLUG, PROJECT, MODULE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/module-links/`
    );
    expect(moduleLinkPath(SLUG, PROJECT, MODULE, LINK)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/module-links/${LINK}/`
    );
    expect(moduleFavoritesPath(SLUG, PROJECT)).toBe(`/api/workspaces/acme/projects/${PROJECT}/user-favorite-modules/`);
    expect(moduleFavoritePath(SLUG, PROJECT, MODULE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/user-favorite-modules/${MODULE}/`
    );
    expect(moduleArchivePath(SLUG, PROJECT, MODULE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/archive/`
    );
    expect(archivedModulesPath(SLUG, PROJECT)).toBe(`/api/workspaces/acme/projects/${PROJECT}/archived-modules/`);
    expect(archivedModulePath(SLUG, PROJECT, MODULE)).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/archived-modules/${MODULE}/`
    );
    expect(workspaceModulesPath(SLUG)).toBe("/api/workspaces/acme/modules/");
  });
});

describe("module reads", () => {
  it("reads the thin list, the project list and the workspace set", async () => {
    const thin = client([{ id: MODULE, project_id: PROJECT, name: "M", status: "planned", is_favorite: false }]);
    expect(await listModules(thin.client, SLUG, PROJECT)).toHaveLength(1);
    expect(requests(thin.calls)[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/`);

    const full = client([ROW]);
    expect((await listProjectModules(full.client, SLUG, PROJECT))[0]?.total_issues).toBe(12);
    expect(requests(full.calls)[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/`);

    const workspace = client([ROW, { ...ROW, id: MODULE_2, project_id: MODULE_2 }]);
    expect(await listWorkspaceModules(workspace.client, SLUG)).toHaveLength(2);
    expect(requests(workspace.calls)[0]?.url).toBe("/api/workspaces/acme/modules/");
  });

  it("reads the module detail", async () => {
    const detail = client(DETAIL);
    expect((await getModule(detail.client, SLUG, PROJECT, MODULE)).distribution.assignees).toHaveLength(2);
    expect(requests(detail.calls)[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/`);
  });

  it("reads module links newest first", async () => {
    const links = client([LINK_ROW]);
    expect(await listModuleLinks(links.client, SLUG, PROJECT, MODULE)).toHaveLength(1);
    expect(requests(links.calls)[0]?.url).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/module-links/`
    );
  });
});

describe("module writes", () => {
  it("creates a module with only the fields the form sent", async () => {
    const created = client(ROW, 201);
    const result = await createModule(created.client, SLUG, PROJECT, {
      name: "Checkout revamp",
      status: "in-progress",
      start_date: null,
      target_date: "2026-10-31",
    });
    expect(result.id).toBe(MODULE);
    const call = requests(created.calls)[0];
    expect(call?.init.method).toBe("POST");
    expect(call?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/`);
    expect(JSON.parse(call?.init.body ?? "{}")).toEqual({
      name: "Checkout revamp",
      status: "in-progress",
      start_date: null,
      target_date: "2026-10-31",
    });
  });

  it("patches only the changed field and answers the update echo", async () => {
    const updated = client({
      id: MODULE,
      name: "Checkout revamp",
      project_id: PROJECT,
      description: "",
      start_date: "2026-09-01",
      target_date: null,
      status: "paused",
    });
    const result = await updateModule(updated.client, SLUG, PROJECT, MODULE, { status: "paused" });
    expect(result.status).toBe("paused");
    expect(requests(updated.calls)[0]?.init.method).toBe("PATCH");
    expect(JSON.parse(requests(updated.calls)[0]?.init.body ?? "{}")).toEqual({ status: "paused" });
  });

  it("patches the manual rank alone for a timeline reorder", async () => {
    const updated = client({
      id: MODULE,
      name: "Checkout revamp",
      project_id: PROJECT,
      description: "",
      start_date: "2026-09-01",
      target_date: null,
      status: "paused",
    });
    const result = await updateModule(updated.client, SLUG, PROJECT, MODULE, { sort_order: 1000 });
    expect(result.id).toBe(MODULE);
    expect(requests(updated.calls)[0]?.init.method).toBe("PATCH");
    expect(JSON.parse(requests(updated.calls)[0]?.init.body ?? "{}")).toEqual({ sort_order: 1000 });
  });

  it("replaces a module with the full body and reads the write echo", async () => {
    const replaced = client(PUT_ECHO);
    const input = {
      name: "Checkout revamp",
      description: "Rework",
      start_date: "2026-09-01",
      target_date: "2026-10-31",
      status: "in-progress" as const,
      lead_id: USER,
      member_ids: [USER],
    };
    const echo = await replaceModule(replaced.client, SLUG, PROJECT, MODULE, input);
    expect(echo.id).toBe(MODULE);
    expect(echo.project).toBe(PROJECT);
    expect(echo.member_ids).toEqual([USER]);
    expect(requests(replaced.calls)[0]?.init.method).toBe("PUT");
    expect(requests(replaced.calls)[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/`);
    expect(JSON.parse(requests(replaced.calls)[0]?.init.body ?? "{}")).toEqual(input);
  });

  it("rejects a PUT answer that is a read row rather than the write echo", async () => {
    // A list row has the annotations but not the write echo's relation keys;
    // parsing it as the echo must fail instead of silently mistyping it.
    const { project: _project, ...withoutProject } = PUT_ECHO;
    void _project;
    const wrong = client(withoutProject);
    await expect(
      replaceModule(wrong.client, SLUG, PROJECT, MODULE, {
        name: "Checkout revamp",
        status: "in-progress",
        lead_id: USER,
        member_ids: [USER],
      })
    ).rejects.toThrow();
  });

  it("rejects a write echo with an unknown status or a malformed date", async () => {
    await expect(
      replaceModule(client({ ...PUT_ECHO, status: "archived" }).client, SLUG, PROJECT, MODULE, {
        name: "Checkout revamp",
      })
    ).rejects.toThrow();
    await expect(
      replaceModule(client({ ...PUT_ECHO, target_date: "31/10/2026" }).client, SLUG, PROJECT, MODULE, {
        name: "Checkout revamp",
      })
    ).rejects.toThrow();
  });

  it("deletes a module and detaches a work item", async () => {
    const removed = client(undefined);
    await deleteModule(removed.client, SLUG, PROJECT, MODULE);
    await removeIssueFromModule(removed.client, SLUG, PROJECT, MODULE, ISSUE);
    expect(requests(removed.calls).map((call) => [call.init.method, call.url])).toEqual([
      ["DELETE", `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/`],
      ["DELETE", `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/issues/${ISSUE}/`],
    ]);
  });

  it("exposes the work-item attach path the Issues contract posts to", async () => {
    // Reuse is deliberate: `addIssuesToModule` (issue-actions.ts) owns the
    // POST .../modules/{moduleId}/issues/ write; this builder must match it.
    const { addIssuesToModule } = await import("./issue-actions.js");
    const added = client({ message: "success" }, 201);
    await addIssuesToModule(added.client, SLUG, PROJECT, MODULE, [ISSUE, MODULE_2]);
    expect(requests(added.calls)[0]?.url).toBe(moduleIssuesPath(SLUG, PROJECT, MODULE));
    expect(JSON.parse(requests(added.calls)[0]?.init.body ?? "{}")).toEqual({ issues: [ISSUE, MODULE_2] });

    // The work-item-side picker attaches through the issue's own surface;
    // the module area contracts the body but leaves the writer to Issues.
    const { setIssueModules } = await import("./issue-actions.js");
    const picked = client({ message: "success" });
    await setIssueModules(picked.client, SLUG, PROJECT, ISSUE, ModuleMembershipInput.parse({ modules: [MODULE] }));
    expect(requests(picked.calls)[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/issues/${ISSUE}/modules/`);
    expect(JSON.parse(requests(picked.calls)[0]?.init.body ?? "{}")).toEqual({ modules: [MODULE] });
  });

  it("creates, updates and deletes links", async () => {
    const created = client(LINK_ROW, 201);
    expect((await createModuleLink(created.client, SLUG, PROJECT, MODULE, { url: "example.test/design" })).id).toBe(
      LINK
    );
    expect(requests(created.calls)[0]?.init.method).toBe("POST");
    expect(JSON.parse(requests(created.calls)[0]?.init.body ?? "{}")).toEqual({ url: "example.test/design" });

    const updated = client(LINK_ROW);
    await updateModuleLink(updated.client, SLUG, PROJECT, MODULE, LINK, {
      url: "https://example.test/design",
      title: "Design doc",
    });
    expect(requests(updated.calls)[0]?.init.method).toBe("PATCH");
    expect(requests(updated.calls)[0]?.url).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/module-links/${LINK}/`
    );
    expect(JSON.parse(requests(updated.calls)[0]?.init.body ?? "{}")).toEqual({
      url: "https://example.test/design",
      title: "Design doc",
    });

    const deleted = client(undefined);
    await deleteModuleLink(deleted.client, SLUG, PROJECT, MODULE, LINK);
    expect(requests(deleted.calls)[0]?.init.method).toBe("DELETE");
    expect(requests(deleted.calls)[0]?.url).toBe(
      `/api/workspaces/acme/projects/${PROJECT}/modules/${MODULE}/module-links/${LINK}/`
    );
  });

  it("favorites and unfavorites through the module favorite collection", async () => {
    const writes = client(undefined);
    await favoriteModule(writes.client, SLUG, PROJECT, MODULE);
    await unfavoriteModule(writes.client, SLUG, PROJECT, MODULE);
    expect(requests(writes.calls).map((call) => [call.init.method, call.url])).toEqual([
      ["POST", `/api/workspaces/acme/projects/${PROJECT}/user-favorite-modules/`],
      ["DELETE", `/api/workspaces/acme/projects/${PROJECT}/user-favorite-modules/${MODULE}/`],
    ]);
    expect(JSON.parse(requests(writes.calls)[0]?.init.body ?? "{}")).toEqual({ module: MODULE });
  });

  it("leaves an archive refusal unparsed so the caller reports the server's reason", async () => {
    const refused = client({ error: "Only completed or cancelled modules can be archived" }, 400);
    const error = await archiveModule(refused.client, SLUG, PROJECT, MODULE).catch((thrown: unknown) => thrown);

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 400, message: "Only completed or cancelled modules can be archived" });
    const body = ModuleErrorBody.parse((error as ApiError).body);
    expect("error" in body ? body.error : "").toContain("completed or cancelled");
  });
});
