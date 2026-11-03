// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module addresses and the entry inventory (MOD-038, MOD-042, MOD-043):
// two reachable shapes plus the archived peek, one copy-link rule per
// state, and an entry list that is edition-independent by construction.
import { describe, expect, it } from "vitest";

import { moduleDetailHref, modulesListHref } from "./moduleList.js";
import {
  archivedModulePeekHref,
  isModuleEndpointPath,
  MODULE_ENDPOINT_PATTERNS,
  MODULE_ENTRIES,
  MODULE_PEEK_PARAM,
  moduleShareHref,
  modulesPeekHref,
  readPeekModule,
} from "./moduleEntry.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

describe("module addresses (MOD-038)", () => {
  it("keeps the list and detail shapes the copy action produces", () => {
    expect(modulesListHref(WS, PROJECT)).toBe(`/acme/projects/${PROJECT}/modules/`);
    expect(moduleDetailHref(WS, PROJECT, MODULE)).toBe(`/acme/projects/${PROJECT}/modules/${MODULE}`);
  });

  it("adds the peek key to the active list and drops it again when closed", () => {
    expect(MODULE_PEEK_PARAM).toBe("peekModule");
    expect(modulesPeekHref(WS, PROJECT, MODULE)).toBe(`/acme/projects/${PROJECT}/modules?peekModule=${MODULE}`);
    expect(modulesPeekHref(WS, PROJECT, null)).toBe(modulesListHref(WS, PROJECT));
  });

  it("puts the archived peek on the archived screen", () => {
    expect(archivedModulePeekHref(WS, PROJECT, MODULE)).toBe(
      `/acme/projects/${PROJECT}/archives/modules?peekModule=${MODULE}`
    );
    expect(archivedModulePeekHref(WS, PROJECT, null)).toBe(`/acme/projects/${PROJECT}/archives/modules`);
  });

  it("shares a live module as its detail and an archived one as its archived peek", () => {
    expect(moduleShareHref({ workspaceSlug: WS, projectId: PROJECT, moduleId: MODULE, archived: false })).toBe(
      moduleDetailHref(WS, PROJECT, MODULE)
    );
    expect(moduleShareHref({ workspaceSlug: WS, projectId: PROJECT, moduleId: MODULE, archived: true })).toBe(
      archivedModulePeekHref(WS, PROJECT, MODULE)
    );
  });

  it("reads the peek key only when it is a module id", () => {
    expect(readPeekModule({ peekModule: MODULE })).toBe(MODULE);
    expect(readPeekModule({ peekModule: "not-an-id" })).toBeNull();
    expect(readPeekModule({ peekModule: 42 })).toBeNull();
    expect(readPeekModule({})).toBeNull();
    expect(readPeekModule({ layout: "list" })).toBeNull();
  });

  it("encodes the id it writes back", () => {
    expect(modulesPeekHref(WS, PROJECT, "a b")).toBe(`/acme/projects/${PROJECT}/modules?peekModule=a%20b`);
  });
});

describe("module entry inventory (MOD-042, MOD-043)", () => {
  it("lists every address with its peek key, and nothing build-specific", () => {
    expect(MODULE_ENTRIES.map((entry) => entry.id)).toEqual([
      "list",
      "peek",
      "detail",
      "archive",
      "archived-peek",
      "settings-switch",
    ]);
    expect(MODULE_ENTRIES.filter((entry) => entry.peekKey).map((entry) => entry.id)).toEqual(["peek", "archived-peek"]);
    for (const entry of MODULE_ENTRIES) {
      // An entry is a path and a peek key; anyone adding a build or edition
      // dimension has to change this shape, which the audit below rejects.
      expect(Object.keys(entry).sort()).toEqual(["id", "path", "peekKey"]);
      expect(entry.path.startsWith("/{ws}/")).toBe(true);
    }
    expect(Object.isFrozen(MODULE_ENTRIES)).toBe(true);
  });

  it("names no edition or desktop marker in any entry", () => {
    for (const entry of MODULE_ENTRIES) {
      expect(entry.path).not.toMatch(/desktop|\bee\b|cloud|license|billing|premium|upgrade|offline/i);
    }
  });

  it("keeps entry traffic on the known module endpoints", () => {
    const known = [
      "/api/workspaces/acme/projects/p/modules/",
      `/api/workspaces/acme/projects/p/modules/${MODULE}/`,
      "/api/workspaces/acme/projects/p/archived-modules/",
      `/api/workspaces/acme/projects/p/archived-modules/${MODULE}/`,
      `/api/workspaces/acme/projects/p/modules/${MODULE}/module-links/`,
      "/api/workspaces/acme/user-favorite-modules/",
      `/api/workspaces/acme/projects/p/modules/${MODULE}/issues/`,
    ];
    for (const path of known) expect(isModuleEndpointPath(path), path).toBe(true);
    const foreign = [
      "/api/workspaces/acme/projects/p/",
      "/api/workspaces/acme/projects/p/issues/",
      "/api/billing/subscriptions/",
      "/api/workspaces/acme/projects/p/modules/premium/",
    ];
    for (const path of foreign) expect(isModuleEndpointPath(path), path).toBe(false);
  });

  it("pins the endpoint patterns as frozen data", () => {
    expect(Object.isFrozen(MODULE_ENDPOINT_PATTERNS)).toBe(true);
    expect(MODULE_ENDPOINT_PATTERNS.length).toBeGreaterThan(0);
  });
});
