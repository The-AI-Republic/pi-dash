// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isPlausibleSpaceEmail, isStrongSpacePassword, spacePasswordCriteria } from "./spaceAuthValidation.js";

describe("isPlausibleSpaceEmail", () => {
  it.each([
    ["ada@example.com", true],
    ["  Ada@Example.COM  ", true],
    ["first.last@mail.example.co", true],
    ["tag+filter@example-domain.com", true],
    ["a@b.co", true],
    ["", false],
    ["   ", false],
    ["not-an-email", false],
    ["@example.com", false],
    ["ada@", false],
    ["a@b@c.com", false],
    ["a b@example.com", false],
    ["ada@exam ple.com", false],
    [".ada@example.com", false],
    ["ada.@example.com", false],
    ["a..d@example.com", false],
    ["ada@example", false],
    ["ada@.com", false],
    ["ada@example.", false],
    ["ada@b.c", false],
    ["ada@example.c0m", false],
    ["ada@exam!ple.com", false],
    ["ada@exam..com", false],
  ])("classifies %j as %s", (value, expected) => {
    expect(isPlausibleSpaceEmail(value)).toBe(expected);
  });

  it("rejects overlong values", () => {
    expect(isPlausibleSpaceEmail(`${"a".repeat(250)}@ex.com`)).toBe(false);
    expect(isPlausibleSpaceEmail(`${"a".repeat(65)}@ex.com`)).toBe(false);
  });
});

describe("spacePasswordCriteria", () => {
  it("reports every rule for a strong secret", () => {
    expect(spacePasswordCriteria("Str0ng!Pass1")).toEqual([
      { id: "length", label: "8+ characters", met: true },
      { id: "upper", label: "Uppercase letter", met: true },
      { id: "lower", label: "Lowercase letter", met: true },
      { id: "digit", label: "Number", met: true },
      { id: "symbol", label: "Symbol", met: true },
    ]);
  });

  it("meets nothing for the empty secret", () => {
    expect(spacePasswordCriteria("").every((criterion) => !criterion.met)).toBe(true);
  });

  it("requires eight characters", () => {
    expect(spacePasswordCriteria("Aa1!bbb")[0]?.met).toBe(false);
    expect(spacePasswordCriteria("Aa1!bbbb")[0]?.met).toBe(true);
  });

  it.each([
    ["abcdefgh1!", "upper", false],
    ["ABCDEFGH1!", "lower", false],
    ["Abcdefgh!!", "digit", false],
    ["Abcdefgh12", "symbol", false],
  ])("flags the missing %s class", (password, id, met) => {
    const criterion = spacePasswordCriteria(password).find((entry) => entry.id === id);
    expect(criterion?.met).toBe(met);
  });

  it.each([
    ["!", '"', "#", "$", "%", "&", "'", "(", ")", "*", "+", ",", "-", ".", "/"],
    [":", ";", "<", "=", ">", "?", "@"],
    ["[", "]", "^", "_"],
    ["{", "|", "}"],
  ])("accepts punctuation marks as symbols", (...marks) => {
    for (const mark of marks) {
      const criterion = spacePasswordCriteria(`Abcdefg1${mark}`).find((entry) => entry.id === "symbol");
      expect(criterion?.met).toBe(true);
    }
  });

  it.each([" ", "\\", "`", "~", "é"])("never counts %j as a symbol", (mark) => {
    const criterion = spacePasswordCriteria(`Abcdefg1${mark}`).find((entry) => entry.id === "symbol");
    expect(criterion?.met).toBe(false);
  });
});

describe("isStrongSpacePassword", () => {
  it("passes strong secrets and rejects the rest", () => {
    expect(isStrongSpacePassword("Str0ng!Pass1")).toBe(true);
    expect(isStrongSpacePassword("abc")).toBe(false);
    expect(isStrongSpacePassword("abcdefgh")).toBe(false);
    expect(isStrongSpacePassword("")).toBe(false);
  });
});
