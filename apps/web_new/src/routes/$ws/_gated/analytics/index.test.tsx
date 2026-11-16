// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare analytics address (parity/specs/analytics.md §1, ANL-001):
// forwards to the overview tab with the query string kept.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

async function thrownFrom(run: () => unknown): Promise<unknown> {
  try {
    await run();
  } catch (error) {
    return error;
  }
  return null;
}

describe("bare analytics address", () => {
  it("redirects to the overview tab", async () => {
    const thrown = await thrownFrom(() =>
      Route.options.beforeLoad?.({ location: { searchStr: "" }, params: { ws: "acme" } } as never)
    );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/acme/analytics/overview");
  });

  it("keeps the query string across the redirect", async () => {
    const thrown = await thrownFrom(() =>
      Route.options.beforeLoad?.({
        location: { searchStr: "?project_ids=a%2Cb" },
        params: { ws: "acme" },
      } as never)
    );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/acme/analytics/overview?project_ids=a%2Cb");
  });
});
