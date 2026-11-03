// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";
import { resetRegisteredStores } from "../../core/session/store.js";
import { useShortcutsDialog } from "./shortcutsDialogStore.js";

afterEach(() => {
  useShortcutsDialog.getState().closeShortcuts();
});

describe("shortcuts dialog store", () => {
  it("opens, closes and resets on sign-out", () => {
    expect(useShortcutsDialog.getState().open).toBe(false);
    useShortcutsDialog.getState().openShortcuts();
    expect(useShortcutsDialog.getState().open).toBe(true);
    useShortcutsDialog.getState().closeShortcuts();
    expect(useShortcutsDialog.getState().open).toBe(false);
    useShortcutsDialog.getState().openShortcuts();
    resetRegisteredStores();
    expect(useShortcutsDialog.getState().open).toBe(false);
  });
});
