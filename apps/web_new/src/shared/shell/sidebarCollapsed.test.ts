// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";
import { resetRegisteredStores } from "../../core/session/store.js";
import { useSidebarCollapsed } from "./sidebarCollapsed.js";

afterEach(() => {
  useSidebarCollapsed.setState({ collapsed: false });
});

describe("sidebar collapse flag", () => {
  it("toggles and sets", () => {
    expect(useSidebarCollapsed.getState().collapsed).toBe(false);
    useSidebarCollapsed.getState().toggleCollapsed();
    expect(useSidebarCollapsed.getState().collapsed).toBe(true);
    useSidebarCollapsed.getState().setCollapsed(false);
    expect(useSidebarCollapsed.getState().collapsed).toBe(false);
  });

  it("resets on sign-out", () => {
    useSidebarCollapsed.getState().setCollapsed(true);
    resetRegisteredStores();
    expect(useSidebarCollapsed.getState().collapsed).toBe(false);
  });
});
