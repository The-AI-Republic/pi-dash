// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { SearchResultHit } from "../../../shared/commands/index.js";
import { resolveSearchTarget } from "./searchTarget.js";

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const ISSUE_ID = "22222222-2222-4222-8222-222222222222";
const CYCLE_ID = "33333333-3333-4333-8333-333333333333";
const MODULE_ID = "44444444-4444-4444-8444-444444444444";
const VIEW_ID = "55555555-5555-4555-8555-555555555555";
const PAGE_ID = "66666666-6666-4666-8666-666666666666";
const OTHER_PROJECT_ID = "77777777-7777-4777-8777-777777777777";

function hit(entity: SearchResultHit["entity"], raw: unknown): SearchResultHit {
  return { key: `${entity}:x`, entity, title: "Hit", path: "", raw };
}

describe("resolveSearchTarget", () => {
  it("sends issues and intake items to the browse route", () => {
    const row = { id: ISSUE_ID, sequence_id: 12, project__identifier: "ATL", project_id: PROJECT_ID };
    expect(resolveSearchTarget(hit("issue", row), "acme")).toEqual({
      to: "/$ws/browse/$issueKey",
      params: { ws: "acme", issueKey: "ATL-12" },
    });
    expect(resolveSearchTarget(hit("intake", row), "acme")).toEqual({
      to: "/$ws/browse/$issueKey",
      params: { ws: "acme", issueKey: "ATL-12" },
    });
  });

  it("falls back to the project tab when the issue key is unreadable", () => {
    expect(resolveSearchTarget(hit("issue", { id: ISSUE_ID, project_id: PROJECT_ID }), "acme")).toEqual({
      to: "/$ws/projects/$projectId/issues",
      params: { ws: "acme", projectId: PROJECT_ID },
    });
    expect(
      resolveSearchTarget(
        hit("issue", { id: ISSUE_ID, sequence_id: "twelve", project__identifier: "ATL", project_id: PROJECT_ID }),
        "acme"
      )
    ).toEqual({
      to: "/$ws/projects/$projectId/issues",
      params: { ws: "acme", projectId: PROJECT_ID },
    });
  });

  it("sends projects to their tracking tab", () => {
    expect(resolveSearchTarget(hit("project", { id: PROJECT_ID }), "acme")).toEqual({
      to: "/$ws/projects/$projectId/issues",
      params: { ws: "acme", projectId: PROJECT_ID },
    });
  });

  it("sends workspaces home under their own slug", () => {
    expect(resolveSearchTarget(hit("workspace", { slug: "globex" }), "acme")).toEqual({
      to: "/$ws",
      params: { ws: "globex" },
    });
    expect(resolveSearchTarget(hit("workspace", {}), "acme")).toEqual({
      to: "/$ws",
      params: { ws: "acme" },
    });
  });

  it("sends cycle and module hits to their entity pages", () => {
    expect(resolveSearchTarget(hit("cycle", { id: CYCLE_ID, project_id: PROJECT_ID }), "acme")).toEqual({
      to: "/$ws/projects/$projectId/cycles/$cycleId",
      params: { ws: "acme", projectId: PROJECT_ID, cycleId: CYCLE_ID },
    });
    expect(resolveSearchTarget(hit("module", { id: MODULE_ID, project_id: PROJECT_ID }), "acme")).toEqual({
      to: "/$ws/projects/$projectId/modules/$moduleId",
      params: { ws: "acme", projectId: PROJECT_ID, moduleId: MODULE_ID },
    });
  });

  it("sends project view hits to the project view and project-less views to the workspace view", () => {
    expect(resolveSearchTarget(hit("issue_view", { id: VIEW_ID, project_id: PROJECT_ID }), "acme")).toEqual({
      to: "/$ws/projects/$projectId/views/$viewId",
      params: { ws: "acme", projectId: PROJECT_ID, viewId: VIEW_ID },
    });
    expect(resolveSearchTarget(hit("issue_view", { id: VIEW_ID }), "acme")).toEqual({
      to: "/$ws/workspace-views/$viewId",
      params: { ws: "acme", viewId: VIEW_ID },
    });
  });

  it("sends page hits to their project page, preferring the current project", () => {
    const firstProject = {
      to: "/$ws/projects/$projectId/pages/$pageId",
      params: { ws: "acme", projectId: PROJECT_ID, pageId: PAGE_ID },
    };
    // First listed project when no current project applies.
    expect(resolveSearchTarget(hit("page", { id: PAGE_ID, project_ids: [PROJECT_ID] }), "acme")).toEqual(firstProject);
    expect(resolveSearchTarget(hit("page", { id: PAGE_ID, project_ids: ["", 42, PROJECT_ID] }), "acme")).toEqual(
      firstProject
    );
    // The current project wins when it is in the list, like the old palette.
    expect(
      resolveSearchTarget(
        hit("page", { id: PAGE_ID, project_ids: [PROJECT_ID, OTHER_PROJECT_ID] }),
        "acme",
        OTHER_PROJECT_ID
      )
    ).toEqual({
      to: "/$ws/projects/$projectId/pages/$pageId",
      params: { ws: "acme", projectId: OTHER_PROJECT_ID, pageId: PAGE_ID },
    });
    // A current project outside the list keeps the first entry.
    expect(
      resolveSearchTarget(hit("page", { id: PAGE_ID, project_ids: [PROJECT_ID] }), "acme", OTHER_PROJECT_ID)
    ).toEqual(firstProject);
    // An explicit null current project behaves like an absent one.
    expect(resolveSearchTarget(hit("page", { id: PAGE_ID, project_ids: [PROJECT_ID] }), "acme", null)).toEqual(
      firstProject
    );
  });

  it("falls back home when the row is unreadable", () => {
    const home = { to: "/$ws", params: { ws: "acme" } };
    expect(resolveSearchTarget(hit("cycle", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("cycle", { id: CYCLE_ID }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("cycle", { project_id: PROJECT_ID }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("module", { id: MODULE_ID }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("issue_view", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("project", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("project", { id: "" }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("issue", undefined), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("issue", { id: 42 }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { id: PAGE_ID }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: [] }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: [42, ""] }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: "nope" }), "acme")).toEqual(home);
  });
});
