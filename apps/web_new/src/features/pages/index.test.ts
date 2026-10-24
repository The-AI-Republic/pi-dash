// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The pages barrel exposes the list shell's public surface for later slices.

import { describe, expect, it } from "vitest";

import * as pages from "./index.js";
import { pageKeys } from "./api/keys.js";

describe("pages barrel", () => {
  it("exports the list model, components and query helpers", () => {
    expect(pages.DEFAULT_PAGE_SORT).toEqual({ key: "modified", direction: "descending" });
    expect(pages.parsePageListSearch({ type: "private" })).toEqual({ type: "private" });
    expect(typeof pages.PagesListScreen).toBe("function");
    expect(typeof pages.PageRow).toBe("function");
    expect(typeof pages.copyPageLink).toBe("function");
    expect(pages.pageKeys).toBe(pageKeys);
  });

  it("scopes collection and detail keys under one workspace pages root", () => {
    expect(pageKeys.all("acme")).toContain("pages");
    expect(pageKeys.list("acme", "project-1")).toEqual([...pageKeys.all("acme"), "list", "project-1"]);
    expect(pageKeys.detail("acme", "project-1", "page-1")).toEqual([
      ...pageKeys.all("acme"),
      "detail",
      "project-1",
      "page-1",
    ]);
  });
});
