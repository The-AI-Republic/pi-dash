// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";
import { safeNextPath } from "./sign-in.js";

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
