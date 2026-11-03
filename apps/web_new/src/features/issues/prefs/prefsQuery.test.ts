// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import { issueKeys } from "../api/keys.js";
import type { IssuesEntity } from "./entities.js";
import { entityPrefsKeyPart, prefsQueryOptions, readEntityPrefs } from "./prefsQuery.js";

const PROJECT: IssuesEntity = { scope: "project", projectId: "pid" };
const CYCLE: IssuesEntity = { scope: "cycle", projectId: "pid", cycleId: "cid" };
const MODULE: IssuesEntity = { scope: "module", projectId: "pid", moduleId: "mid" };
const EPICS: IssuesEntity = { scope: "epics", projectId: "pid" };
const WORKSPACE: IssuesEntity = { scope: "workspace" };

function stubClient(calls: { method: string; url: string }[]): ApiClient {
  return {
    getJson: (async (url: string) => {
      calls.push({ method: "GET", url });
      return {};
    }) as ApiClient["getJson"],
    parse: ((_schema: unknown, value: unknown) => value) as ApiClient["parse"],
  } as ApiClient;
}

describe("entity prefs keys", () => {
  it("parts prefs keys per entity", () => {
    expect(entityPrefsKeyPart(PROJECT)).toEqual(["project", "pid"]);
    expect(entityPrefsKeyPart(CYCLE)).toEqual(["cycle", "pid", "cid"]);
    expect(entityPrefsKeyPart(MODULE)).toEqual(["module", "pid", "mid"]);
    expect(entityPrefsKeyPart(EPICS)).toEqual(["epics", "pid"]);
    expect(entityPrefsKeyPart(WORKSPACE)).toEqual(["workspace"]);
  });
});

describe("entity preference reads", () => {
  it.each([
    [PROJECT, "/api/workspaces/acme/projects/pid/user-properties/"],
    [CYCLE, "/api/workspaces/acme/projects/pid/cycles/cid/user-properties/"],
    [MODULE, "/api/workspaces/acme/projects/pid/modules/mid/user-properties/"],
    [EPICS, "/api/workspaces/acme/projects/pid/epics-user-properties/"],
    [WORKSPACE, "/api/workspaces/acme/user-properties/"],
  ] as Array<[IssuesEntity, string]>)(`reads %s through its user-properties route`, async (entity, url) => {
    const calls: { method: string; url: string }[] = [];
    await readEntityPrefs(stubClient(calls), "acme", entity);
    expect(calls).toEqual([{ method: "GET", url }]);
  });
});

describe("prefsQueryOptions", () => {
  it("keys by entity and never revalidates on focus", () => {
    const options = prefsQueryOptions({} as never, "acme", PROJECT);
    expect(options.queryKey).toEqual(issueKeys.prefs("acme", "project", "pid"));
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
  });
});
