// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isClientSideClick, isHomeRowActive } from "./sidebarLink.js";

describe("isClientSideClick", () => {
  it("takes over plain left clicks", () => {
    expect(isClientSideClick({ button: 0, metaKey: false, ctrlKey: false })).toBe(true);
  });

  it("leaves modified left clicks native", () => {
    expect(isClientSideClick({ button: 0, metaKey: true, ctrlKey: false })).toBe(false);
    expect(isClientSideClick({ button: 0, metaKey: false, ctrlKey: true })).toBe(false);
  });

  it("takes over non-left buttons", () => {
    expect(isClientSideClick({ button: 1, metaKey: true, ctrlKey: true })).toBe(true);
  });
});

describe("isHomeRowActive", () => {
  it("lights home on the bare workspace address", () => {
    expect(isHomeRowActive("/acme", "acme")).toBe(true);
    expect(isHomeRowActive("/acme/", "acme")).toBe(true);
    expect(isHomeRowActive("/acme/drafts/", "acme")).toBe(false);
    expect(isHomeRowActive("/", "acme")).toBe(false);
  });
});
