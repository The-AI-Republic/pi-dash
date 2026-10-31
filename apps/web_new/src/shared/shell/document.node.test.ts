// @vitest-environment node
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { OverlayPortal, markInitialColorScheme } from "./document.js";

describe("document shell without a DOM", () => {
  it("marks the default scheme without touching the document", () => {
    expect(markInitialColorScheme()).toBe("light");
  });

  it("renders no portal without a host document", () => {
    expect(OverlayPortal({ rootId: "context-menu-portal", children: "menu" })).toBeNull();
  });
});
