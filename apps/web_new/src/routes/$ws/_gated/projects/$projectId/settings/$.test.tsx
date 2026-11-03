// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Retired settings prefix redirect (PSET-003): every old address lands
// on the workspace settings route for the same project and keeps the
// suffix after the id, so the tab the bookmark named opens.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route } from "./$.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function run(pathname: string): unknown {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("retired settings redirect has no beforeLoad");
  try {
    beforeLoad({
      location: { pathname, searchStr: "", hash: "" },
      params: { ws: "acme", projectId: PROJECT_ID },
    } as never);
  } catch (error) {
    return error;
  }
  throw new Error("redirect did not fire");
}

function hrefOf(thrown: unknown): string | undefined {
  return (thrown as { options?: { href?: string } }).options?.href;
}

describe("retired project settings prefix", () => {
  it("sends the bare settings address to General", () => {
    const thrown = run(`/acme/projects/${PROJECT_ID}/settings`);
    expect(isRedirect(thrown)).toBe(true);
    expect(hrefOf(thrown)).toBe(`/acme/settings/projects/${PROJECT_ID}`);
  });

  it("carries a trailing slash and a tab suffix across", () => {
    const members = run(`/acme/projects/${PROJECT_ID}/settings/members`);
    expect(isRedirect(members)).toBe(true);
    expect(hrefOf(members)).toBe(`/acme/settings/projects/${PROJECT_ID}/members/`);

    const nested = run(`/acme/projects/${PROJECT_ID}/settings/features/cycles/`);
    expect(isRedirect(nested)).toBe(true);
    expect(hrefOf(nested)).toBe(`/acme/settings/projects/${PROJECT_ID}/features/cycles/`);
  });

  it("falls back to General for an address outside the retired prefix", () => {
    const thrown = run("/acme/elsewhere");
    expect(isRedirect(thrown)).toBe(true);
    expect(hrefOf(thrown)).toBe(`/acme/settings/projects/${PROJECT_ID}`);
  });
});
