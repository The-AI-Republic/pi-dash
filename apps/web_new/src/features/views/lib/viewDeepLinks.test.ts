// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One contract for every views deep link (VIEW-045, from
// parity/specs/views.md §13): every list row, switcher pick, crumb,
// copy-link and new-tab entry builds its address with these builders,
// and opening such an address lands on the same list or detail state
// (the area gate proves the landing; this pins the construction).
import { describe, expect, it } from "vitest";

import { projectViewHref, projectViewsFeatureHref, projectViewsListHref } from "./projectViews.js";
import { workspaceCustomViewHref, workspaceDefaultViewHref, workspaceViewsListHref } from "./workspaceViews.js";

/** Every list and detail address the surface links to. */
function everyViewsHref(): Record<string, string> {
  return {
    projectDetail: projectViewHref("acme", "project-1", "view-9"),
    projectList: projectViewsListHref("acme", "project-1"),
    projectFeatureLeaf: projectViewsFeatureHref("acme", "project-1"),
    workspaceDefault: workspaceDefaultViewHref("acme", "assigned"),
    workspaceCustom: workspaceCustomViewHref("acme", "view-9"),
    workspaceList: workspaceViewsListHref("acme"),
  };
}

describe("views deep links (VIEW-045)", () => {
  it("builds the canonical address for every list and detail path", () => {
    expect(everyViewsHref()).toEqual({
      projectDetail: "/acme/projects/project-1/views/view-9",
      projectList: "/acme/projects/project-1/views",
      projectFeatureLeaf: "/acme/settings/projects/project-1/features/views",
      workspaceDefault: "/acme/workspace-views/assigned",
      workspaceCustom: "/acme/workspace-views/view-9",
      workspaceList: "/acme/workspace-views",
    });
  });

  it("keeps every address a clean in-app path", () => {
    for (const [name, href] of Object.entries(everyViewsHref())) {
      expect(href.startsWith("/"), `${name} leads with a slash`).toBe(true);
      expect(href.endsWith("/"), `${name} trails no slash`).toBe(false);
      expect(href.includes("//"), `${name} doubles no slash`).toBe(false);
      expect(href.includes("?") || href.includes("#"), `${name} carries no query or hash`).toBe(false);
    }
  });
});
