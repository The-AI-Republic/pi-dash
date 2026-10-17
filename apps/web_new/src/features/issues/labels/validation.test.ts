// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  LABEL_NAME_MAX_LENGTH,
  LABEL_NAME_REQUIRED_MESSAGE,
  LABEL_NAME_TOO_LONG_MESSAGE,
  validateLabelName,
} from "./validation.js";

describe("validateLabelName", () => {
  it("requires a non-blank name", () => {
    expect(validateLabelName("")).toBe(LABEL_NAME_REQUIRED_MESSAGE);
    expect(validateLabelName("   ")).toBe(LABEL_NAME_REQUIRED_MESSAGE);
    expect(LABEL_NAME_REQUIRED_MESSAGE).toBe("Label title is required");
  });

  it("caps the name at 255 characters", () => {
    expect(validateLabelName("x".repeat(LABEL_NAME_MAX_LENGTH))).toBe(null);
    expect(validateLabelName("x".repeat(LABEL_NAME_MAX_LENGTH + 1))).toBe(LABEL_NAME_TOO_LONG_MESSAGE);
    expect(LABEL_NAME_TOO_LONG_MESSAGE).toBe("Label name should not exceed 255 characters");
  });

  it("accepts an ordinary name", () => {
    expect(validateLabelName("Triage")).toBe(null);
  });
});
