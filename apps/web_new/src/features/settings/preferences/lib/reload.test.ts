// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reload seam for the custom designer (parity/specs/profile.md §7, PROF-016).
import { afterEach, describe, expect, it, vi } from "vitest";

import { reloadDocument } from "./reload.js";

afterEach(() => vi.unstubAllGlobals());

describe("reloadDocument", () => {
  it("re-enters the running document", () => {
    const reload = vi.fn();
    vi.stubGlobal("window", { location: { reload } });
    reloadDocument();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("is a no-op where no window exists", () => {
    vi.stubGlobal("window", undefined);
    expect(() => reloadDocument()).not.toThrow();
  });
});
