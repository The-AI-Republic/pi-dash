// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section entry (parity/specs/agents.md §5, AGT-021):
// the bare tab address redirects to the calendar, which is the section's
// default landing.
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

describe("schedulers section index", () => {
  it("redirects the bare address to the calendar tab", () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("schedulers index route has no beforeLoad");
    const params = { ws: "acme", projectId: "p1" };
    let thrown: unknown;
    try {
      beforeLoad({ params } as never);
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Response);
    const response = thrown as Response;
    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe("/acme/projects/p1/schedulers/calendar");
  });
});
