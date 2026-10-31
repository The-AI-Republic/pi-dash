// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";
import { resetRegisteredStores } from "../../core/session/store.js";
import { useTopSearchFocus } from "./topSearchFocus.js";

afterEach(() => {
  useTopSearchFocus.setState({ request: 0 });
});

describe("top-search focus requests", () => {
  it("bumps the counter on every request", () => {
    expect(useTopSearchFocus.getState().request).toBe(0);
    useTopSearchFocus.getState().requestTopSearchFocus();
    useTopSearchFocus.getState().requestTopSearchFocus();
    expect(useTopSearchFocus.getState().request).toBe(2);
  });

  it("resets on sign-out", () => {
    useTopSearchFocus.getState().requestTopSearchFocus();
    resetRegisteredStores();
    expect(useTopSearchFocus.getState().request).toBe(0);
  });
});
