// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { asString, safeNextPath } from "./entrySearch.js";

describe("asString", () => {
  it("keeps non-empty strings", () => {
    expect(asString("ada@example.com")).toBe("ada@example.com");
  });

  it("drops empty and non-string values", () => {
    expect(asString("")).toBeUndefined();
    expect(asString(undefined)).toBeUndefined();
    expect(asString(null)).toBeUndefined();
    expect(asString(42)).toBeUndefined();
    expect(asString(["/acme"])).toBeUndefined();
  });
});

describe("safeNextPath", () => {
  it("keeps internal return paths", () => {
    expect(safeNextPath("/contract-acme")).toBe("/contract-acme");
    expect(safeNextPath("/a/projects/b/issues?layout=compact")).toBe("/a/projects/b/issues?layout=compact");
  });

  it("drops external, protocol-relative and empty values", () => {
    expect(safeNextPath("https://evil.test/")).toBeUndefined();
    expect(safeNextPath("//evil.test/")).toBeUndefined();
    expect(safeNextPath("")).toBeUndefined();
    expect(safeNextPath(undefined)).toBeUndefined();
    expect(safeNextPath(42)).toBeUndefined();
  });
});
