// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import { entityListsKeyPart, writeEntityPrefs, type IssuesEntity } from "./entities.js";

const PROJECT: IssuesEntity = { scope: "project", projectId: "pid" };
const CYCLE: IssuesEntity = { scope: "cycle", projectId: "pid", cycleId: "cid" };
const MODULE: IssuesEntity = { scope: "module", projectId: "pid", moduleId: "mid" };
const EPICS: IssuesEntity = { scope: "epics", projectId: "pid" };
const WORKSPACE: IssuesEntity = { scope: "workspace" };

function stubClient(calls: { method: string; url: string; json?: unknown }[]): ApiClient {
  return {
    sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
      calls.push({ method, url, json: options.json });
      return { data: {}, status: 201, url };
    }) as ApiClient["sendJson"],
    parse: ((_schema: unknown, value: unknown) => value) as ApiClient["parse"],
  } as ApiClient;
}

describe("entity lists keys", () => {
  it("parts list prefixes per entity, project matching the flat list", () => {
    expect(entityListsKeyPart(PROJECT)).toEqual(["pid"]);
    expect(entityListsKeyPart(CYCLE)).toEqual(["cycle", "pid", "cid"]);
    expect(entityListsKeyPart(MODULE)).toEqual(["module", "pid", "mid"]);
    expect(entityListsKeyPart(EPICS)).toEqual(["epics", "pid"]);
    expect(entityListsKeyPart(WORKSPACE)).toEqual(["workspace"]);
  });
});

describe("entity preference writes", () => {
  it.each([
    [PROJECT, "/api/workspaces/acme/projects/pid/user-properties/"],
    [CYCLE, "/api/workspaces/acme/projects/pid/cycles/cid/user-properties/"],
    [MODULE, "/api/workspaces/acme/projects/pid/modules/mid/user-properties/"],
    [EPICS, "/api/workspaces/acme/projects/pid/epics-user-properties/"],
    [WORKSPACE, "/api/workspaces/acme/user-properties/"],
  ] as Array<[IssuesEntity, string]>)(`patches %s through its user-properties route`, async (entity, url) => {
    const calls: { method: string; url: string; json?: unknown }[] = [];
    const patch = { display_filters: { layout: "kanban" } };
    await writeEntityPrefs(stubClient(calls), "acme", entity, patch);
    expect(calls).toEqual([{ method: "PATCH", url, json: patch }]);
  });
});
