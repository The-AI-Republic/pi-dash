// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_SCHEDULER_COLOR,
  deriveSchedulerHandle,
  emptySchedulerDefinition,
  hasSchedulerDefinitionErrors,
  validateSchedulerDefinition,
} from "./definition.js";

describe("scheduler definition rules", () => {
  it("creates the enabled blank definition", () => {
    expect(emptySchedulerDefinition()).toEqual({
      name: "",
      slug: "",
      description: "",
      prompt: "",
      color: DEFAULT_SCHEDULER_COLOR,
      isEnabled: true,
    });
  });

  it.each([
    ["Weekly Report", "weekly-report"],
    ["  Build & Review!  ", "build-review"],
    ["Already---Dashed", "already-dashed"],
    ["!!!", ""],
  ])("derives %j as %j", (name, expected) => {
    expect(deriveSchedulerHandle(name)).toBe(expected);
  });

  it("accepts a complete valid definition", () => {
    const errors = validateSchedulerDefinition({
      name: "Weekly report",
      slug: "weekly-report-2",
      description: "Summary",
      prompt: "Review the week",
      color: "#A1b2C3",
      isEnabled: false,
    });
    expect(errors).toEqual({});
    expect(hasSchedulerDefinitionErrors(errors)).toBe(false);
  });

  it("reports every required or malformed field", () => {
    const errors = validateSchedulerDefinition({
      name: "  ",
      slug: "Bad--Handle",
      description: "",
      prompt: " ",
      color: "red",
      isEnabled: true,
    });
    expect(errors).toEqual({
      name: "Enter a scheduler name.",
      slug: "Use lowercase letters, numbers, and single dashes.",
      prompt: "Enter the base prompt.",
      color: "Choose a six-digit hex color.",
    });
    expect(hasSchedulerDefinitionErrors(errors)).toBe(true);
  });

  it("distinguishes a missing handle from a malformed one", () => {
    expect(validateSchedulerDefinition({ ...emptySchedulerDefinition(), name: "Name", prompt: "Prompt" }).slug).toBe(
      "Enter a handle."
    );
  });
});
