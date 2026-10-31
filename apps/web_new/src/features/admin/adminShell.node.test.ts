// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar collapse persistence without a DOM: storage access degrades
// to an expanded read and a no-op write instead of throwing.
import { describe, expect, it } from "vitest";

import { readStoredSidebarCollapsed, writeStoredSidebarCollapsed } from "./adminShell.js";

describe("sidebar collapse persistence without a DOM", () => {
  it("reads expanded with no store to read", () => {
    expect(readStoredSidebarCollapsed()).toBe(false);
  });

  it("writes nothing with no store to persist to", () => {
    expect(() => writeStoredSidebarCollapsed(true)).not.toThrow();
    expect(readStoredSidebarCollapsed()).toBe(false);
  });
});
