// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";

describe("@pidash/api-client entry", () => {
  it("imports cleanly", async () => {
    const entry = await import("./index");
    expect(entry).toBeDefined();
  });
});
