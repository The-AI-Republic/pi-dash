// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";

import { edition } from "./index.js";
import { edition as ossEdition } from "./oss.js";

describe("edition", () => {
  it("resolves the OSS edition through the index", () => {
    expect(edition).toBe(ossEdition);
    expect(edition.id).toBe("oss");
  });

  it("contributes no cloud surface", () => {
    expect(edition.routes ?? []).toEqual([]);
    expect(edition.sidebar ?? []).toEqual([]);
    expect(edition.settingsSections ?? []).toEqual([]);
    expect(edition.auth ?? []).toEqual([]);
    expect(edition.api ?? []).toEqual([]);
    expect(edition.flags).toEqual({});
    expect(edition.slots ?? {}).toEqual({});
  });
});
