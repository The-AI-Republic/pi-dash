// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { redirectRetiredSignIn } from "./login.js";

interface ThrownRedirect extends Response {
  options: { to?: unknown };
}

describe("redirectRetiredSignIn", () => {
  it("targets the home route", () => {
    let thrown: unknown;
    try {
      redirectRetiredSignIn();
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Response);
    expect((thrown as ThrownRedirect).options.to).toBe("/");
  });
});
