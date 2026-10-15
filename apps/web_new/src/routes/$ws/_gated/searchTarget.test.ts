// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { SearchResultHit } from "../../../shared/commands/index.js";
import { resolveSearchTarget } from "./searchTarget.js";

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const ISSUE_ID = "22222222-2222-4222-8222-222222222222";

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

  it.each(["cycle", "module", "issue_view", "page"] as const)(
    "lands %s hits on the project tab until their area lands",
    (entity) => {
      expect(resolveSearchTarget(hit(entity, { project_id: PROJECT_ID }), "acme")).toEqual({
        to: "/$ws/projects/$projectId/issues",
        params: { ws: "acme", projectId: PROJECT_ID },
      });
    }
  );

  it("lands page hits on their first listed project tab", () => {
    const tab = { to: "/$ws/projects/$projectId/issues", params: { ws: "acme", projectId: PROJECT_ID } };
    expect(resolveSearchTarget(hit("page", { project_ids: [PROJECT_ID] }), "acme")).toEqual(tab);
    expect(resolveSearchTarget(hit("page", { project_ids: ["", 42, PROJECT_ID] }), "acme")).toEqual(tab);
    expect(resolveSearchTarget(hit("page", { project_id: PROJECT_ID, project_ids: [ISSUE_ID] }), "acme")).toEqual(tab);
  });

  it("falls back home when the row is unreadable", () => {
    const home = { to: "/$ws", params: { ws: "acme" } };
    expect(resolveSearchTarget(hit("cycle", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("project", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("project", { id: "" }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("issue", undefined), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("issue", { id: 42 }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", {}), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: [] }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: [42, ""] }), "acme")).toEqual(home);
    expect(resolveSearchTarget(hit("page", { project_ids: "nope" }), "acme")).toEqual(home);
  });
});
