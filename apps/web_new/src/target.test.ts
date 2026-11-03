// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
