// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { resetRegisteredStores } from "../../../core/session/index.js";
import { useDetailUiStore } from "./store.js";

describe("DetailUiStore", () => {
  it("starts expanded and toggles the collapse flag", () => {
    useDetailUiStore.setState({ sidebarCollapsed: false });
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(false);
    useDetailUiStore.getState().setSidebarCollapsed(true);
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(true);
    useDetailUiStore.getState().setSidebarCollapsed(false);
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(false);
  });

  it("resets the flag with every other registered store", () => {
    useDetailUiStore.getState().setSidebarCollapsed(true);
    resetRegisteredStores();
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(false);
  });
});
