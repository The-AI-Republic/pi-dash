// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules fixtures serve every test in the area, so their defaults are
// pinned here: a fixture that drifts from the contract row shape would
// quietly weaken the tests that build on it.

import { describe, expect, it } from "vitest";

import {
  jsonResponse,
  makeModuleRow,
  makeProjectDetails,
  makeProjectRow,
  MODULES_MODULE_ID,
  MODULES_PROJECT_ID,
  MODULES_WS_ID,
} from "./testing.js";

describe("module fixtures", () => {
  it("builds a module row with zeroed counts and the project's ids", () => {
    const row = makeModuleRow();
    expect(row.id).toBe(MODULES_MODULE_ID);
    expect(row.project_id).toBe(MODULES_PROJECT_ID);
    expect(row.workspace_id).toBe(MODULES_WS_ID);
    expect(row.status).toBe("backlog");
    expect(row.total_issues).toBe(0);
    expect(row.is_favorite).toBe(false);
    expect(makeModuleRow({ name: "Billing", status: "completed" })).toMatchObject({
      name: "Billing",
      status: "completed",
    });
  });

  it("builds the thin project row with modules on by default", () => {
    const project = makeProjectRow();
    expect(project["name"]).toBe("Website relaunch");
    expect(project["module_view"]).toBe(true);
    expect(makeProjectRow({ module_view: false })["module_view"]).toBe(false);
  });

  it("builds the rich project row with a timezone", () => {
    const project = makeProjectDetails();
    expect(project.id).toBe(MODULES_PROJECT_ID);
    expect(project.timezone).toBe("UTC");
    expect(makeProjectDetails({ timezone: "Asia/Tokyo" }).timezone).toBe("Asia/Tokyo");
  });

  it("wraps a body as a transport response", async () => {
    const response = jsonResponse("https://api.test/thing", 201, { ok: true });
    expect(response.status).toBe(201);
    expect(response.url).toBe("https://api.test/thing");
    expect(await response.text()).toBe('{"ok":true}');
    expect(response.headers.get("content-type")).toBeNull();
  });
});
