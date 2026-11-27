// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project runners route identity (NEWPIDASH-3).
import { describe, expect, it } from "vitest";

import { isProjectRunnersRouteId, PROJECT_RUNNERS_ROUTE_ID } from "./routeId.js";

describe("isProjectRunnersRouteId", () => {
  it("matches the project runners layout and the routes beneath it", () => {
    expect(isProjectRunnersRouteId(PROJECT_RUNNERS_ROUTE_ID)).toBe(true);
    expect(isProjectRunnersRouteId("/$ws/_gated/projects/$projectId/runners/")).toBe(true);
    expect(isProjectRunnersRouteId("/$ws/_gated/projects/$projectId/runners/chat/$runnerId")).toBe(true);
  });

  it("leaves other project tabs and the workspace runners area alone", () => {
    expect(isProjectRunnersRouteId("/$ws/_gated/projects/$projectId")).toBe(false);
    expect(isProjectRunnersRouteId("/$ws/_gated/projects/$projectId/issues")).toBe(false);
    expect(isProjectRunnersRouteId("/$ws/_gated/projects/$projectId/runnersx")).toBe(false);
    expect(isProjectRunnersRouteId("/$ws/_gated/runners")).toBe(false);
  });
});
