// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isStrongSecret, secretCriteria } from "./passwordStrength.js";

describe("isStrongSecret", () => {
  it("accepts a mixed secret of sufficient length", () => {
    expect(isStrongSecret("Tq7!mZ2@kP9#xR")).toBe(true);
    expect(isStrongSecret("Abcdef1!")).toBe(true);
  });

  it("refuses short secrets", () => {
    expect(isStrongSecret("Aa1!")).toBe(false);
    expect(isStrongSecret("")).toBe(false);
  });

  it("refuses secrets missing a character class", () => {
    expect(isStrongSecret("weakpass1")).toBe(false);
    expect(isStrongSecret("ALLUPPER1!")).toBe(false);
    expect(isStrongSecret("alllower1!")).toBe(false);
    expect(isStrongSecret("NoDigits!!")).toBe(false);
    expect(isStrongSecret("NoPunct12")).toBe(false);
  });

  it("refuses marks outside the fixed punctuation set", () => {
    expect(isStrongSecret("Abcdef12 ")).toBe(false);
    expect(isStrongSecret("Abcdef1~")).toBe(false);
    expect(isStrongSecret("Abcdef1`")).toBe(false);
    expect(isStrongSecret("Abcdef1\\")).toBe(false);
    expect(isStrongSecret("Abcdef1é")).toBe(false);
  });

  it("accepts each bracket mark of the fixed set", () => {
    for (const mark of ["[", "]", "^", "_", "{", "|", "}"]) {
      expect(isStrongSecret(`Abcdef1${mark}`)).toBe(true);
    }
  });

  it("counts ASCII letters only for the case classes", () => {
    expect(isStrongSecret("abcdef1!É")).toBe(false);
    expect(isStrongSecret("ABCDEF1!ß")).toBe(false);
  });
});

describe("secretCriteria", () => {
  it("reports every rule with fresh hints", () => {
    expect(secretCriteria("Tq7!mZ2@kP9#xR").map((criterion) => criterion.met)).toEqual([true, true, true, true, true]);
  });

  it("marks the failed rules on a weak secret", () => {
    const criteria = secretCriteria("weakpass1");
    expect(criteria.map((criterion) => [criterion.id, criterion.met])).toEqual([
      ["length", true],
      ["upper", false],
      ["lower", true],
      ["digit", true],
      ["punctuation", false],
    ]);
    expect(criteria.every((criterion) => criterion.hint.length > 0)).toBe(true);
  });

  it("meets nothing on an empty value", () => {
    expect(secretCriteria("").every((criterion) => !criterion.met)).toBe(true);
  });
});
