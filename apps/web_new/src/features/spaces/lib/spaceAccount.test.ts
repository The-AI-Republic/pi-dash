// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { spaceAccountLabel, type SpaceAccountIdentity } from "./spaceAccount.js";

function identity(overrides: Partial<SpaceAccountIdentity> = {}): SpaceAccountIdentity {
  return { display_name: "", first_name: "", last_name: "", email: "", ...overrides };
}

describe("spaceAccountLabel", () => {
  it("prefers the display name", () => {
    expect(
      spaceAccountLabel(identity({ display_name: "Ada", first_name: "Alice", last_name: "Wong", email: "a@x.test" }))
    ).toBe("Ada");
  });

  it("falls back to first plus last name, never repeating the first", () => {
    expect(spaceAccountLabel(identity({ first_name: "Alice", last_name: "Wong" }))).toBe("Alice Wong");
  });

  it("uses the first name alone when no last name exists", () => {
    expect(spaceAccountLabel(identity({ first_name: "Alice" }))).toBe("Alice");
  });

  it("falls back to the address without any name", () => {
    expect(spaceAccountLabel(identity({ email: "a@x.test" }))).toBe("a@x.test");
  });

  it("falls back to the neutral label without any identity text", () => {
    expect(spaceAccountLabel(identity())).toBe("Account");
  });

  it("treats blank text as missing", () => {
    expect(spaceAccountLabel(identity({ display_name: "  ", first_name: " ", last_name: " ", email: " " }))).toBe(
      "Account"
    );
  });
});
