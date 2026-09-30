// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";

import { resolvePidashTarget } from "./target";

describe("resolvePidashTarget", () => {
  it("selects the desktop bundle only for an exact desktop value", () => {
    expect(resolvePidashTarget("desktop")).toBe("desktop");
  });

  it("defaults everything else to the web bundle", () => {
    expect(resolvePidashTarget(undefined)).toBe("web");
    expect(resolvePidashTarget("")).toBe("web");
    expect(resolvePidashTarget("web")).toBe("web");
    expect(resolvePidashTarget("DESKTOP")).toBe("web");
  });
});
