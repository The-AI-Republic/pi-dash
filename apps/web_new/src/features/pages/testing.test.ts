// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The pages fixtures pin the contract-shaped defaults every pages test uses.

import { describe, expect, it } from "vitest";

import {
  PAGES_PAGE_ID,
  PAGES_PROJECT_ID,
  PAGES_WS_ID,
  jsonResponse,
  makePageRow,
  makeProjectDetails,
  makeProjectRow,
  makeWorkspaceMember,
} from "./testing.js";

describe("pages fixtures", () => {
  it("builds a page row with the default project and open access", () => {
    const row = makePageRow();
    expect(row.id).toBe(PAGES_PAGE_ID);
    expect(row.workspace).toBe(PAGES_WS_ID);
    expect(row.project_ids).toEqual([PAGES_PROJECT_ID]);
    expect(row.access).toBe(0);
    expect(row.archived_at).toBeNull();
    expect(makePageRow({ name: "Plan", access: 1 })).toMatchObject({ name: "Plan", access: 1 });
  });

  it("builds the thin project row with pages on by default", () => {
    expect(makeProjectRow()["page_view"]).toBe(true);
    expect(makeProjectRow({ page_view: false })["page_view"]).toBe(false);
  });

  it("builds the rich project row and the directory member", () => {
    expect(makeProjectDetails().name).toBe("Website relaunch");
    expect(makeProjectDetails({ timezone: "Asia/Tokyo" }).timezone).toBe("Asia/Tokyo");
    const member = makeWorkspaceMember();
    expect((member["member"] as Record<string, unknown>)["display_name"]).toBe("Ada Byron");
    expect(makeWorkspaceMember("user-2", "Zed Shaw")).toMatchObject({ id: "user-2" });
  });

  it("wraps a body as a transport response", async () => {
    const response = jsonResponse("https://api.test/pages", 200, []);
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("[]");
    expect(response.headers.get("content-type")).toBeNull();
  });
});
