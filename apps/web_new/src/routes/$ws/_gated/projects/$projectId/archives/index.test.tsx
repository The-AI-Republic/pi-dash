// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

describe("bare archives address", () => {
  it("redirects to the work-items tab", () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("archives index route has no beforeLoad");
    let thrown: unknown;
    try {
      beforeLoad({ params: { ws: "acme", projectId: "p1" } } as never);
    } catch (error) {
      thrown = error;
    }
    expect(isRedirect(thrown)).toBe(true);
    const options = (thrown as unknown as { options: { to: string; params: unknown } }).options;
    expect(options.to).toBe("/$ws/projects/$projectId/archives/issues");
    expect(options.params).toEqual({ ws: "acme", projectId: "p1" });
  });
});
