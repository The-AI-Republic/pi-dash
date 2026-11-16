// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LEGACY_SIGN_UP_TARGET, LegacySignUpRedirect } from "./sign-up.js";

describe("LegacySignUpRedirect", () => {
  const replace = vi.fn();

  beforeEach(() => {
    replace.mockClear();
    Object.defineProperty(globalThis.window, "location", {
      value: { replace },
      writable: true,
      configurable: true,
    });
  });

  it("forwards to the canonical card with its trailing slash", () => {
    expect(LEGACY_SIGN_UP_TARGET).toBe("/sign-up/");
    render(<LegacySignUpRedirect />);
    expect(replace).toHaveBeenCalledWith("/sign-up/");
  });
});
