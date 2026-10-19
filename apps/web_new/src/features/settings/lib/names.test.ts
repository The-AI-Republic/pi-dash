// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { NAME_MESSAGES, namesAreValid, validateNames } from "./names.js";

const valid = { first_name: "Ada", last_name: "Byron", display_name: "ada.byron" };

describe("general-tab name validation", () => {
  it("accepts the valid names, including an empty optional last name", () => {
    expect(validateNames(valid)).toEqual({});
    expect(validateNames({ ...valid, last_name: "" })).toEqual({});
  });

  it("requires the first and display names", () => {
    expect(validateNames({ ...valid, first_name: "" })).toEqual({ first_name: NAME_MESSAGES.firstRequired });
    expect(validateNames({ ...valid, display_name: "" })).toEqual({ display_name: NAME_MESSAGES.displayRequired });
  });

  it("limits person names to letters, spaces, hyphens and apostrophes", () => {
    expect(validateNames({ ...valid, first_name: "R2D2" })).toEqual({ first_name: NAME_MESSAGES.personCharacters });
    expect(validateNames({ ...valid, last_name: "O'Neil-Smith" })).toEqual({});
    expect(validateNames({ ...valid, first_name: "Zoë" })).toEqual({});
  });

  it("limits display names to letters, numbers, periods, hyphens and underscores", () => {
    expect(validateNames({ ...valid, display_name: "has space" })).toEqual({
      display_name: NAME_MESSAGES.displayCharacters,
    });
    expect(validateNames({ ...valid, display_name: "ada_b.2" })).toEqual({});
  });

  it("caps every field at fifty characters", () => {
    const long = "a".repeat(51);
    expect(validateNames({ ...valid, first_name: long })).toEqual({ first_name: NAME_MESSAGES.tooLong });
    expect(validateNames({ ...valid, last_name: long })).toEqual({ last_name: NAME_MESSAGES.tooLong });
    expect(validateNames({ ...valid, display_name: long })).toEqual({ display_name: NAME_MESSAGES.tooLong });
    expect(validateNames({ ...valid, first_name: "a".repeat(50) })).toEqual({});
  });

  it("reports every invalid field at once and gates submit", () => {
    expect(validateNames({ first_name: "", last_name: "1", display_name: " " })).toEqual({
      first_name: NAME_MESSAGES.firstRequired,
      last_name: NAME_MESSAGES.personCharacters,
      display_name: NAME_MESSAGES.displayCharacters,
    });
    expect(namesAreValid(valid)).toBe(true);
    expect(namesAreValid({ ...valid, first_name: "" })).toBe(false);
  });
});
