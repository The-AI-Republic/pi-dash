/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import { deriveSchedulerSlug, diffSchedulerTemplate } from "@/components/schedulers/definition-helpers";

describe("deriveSchedulerSlug", () => {
  it("lowercases and dashes a plain name", () => {
    expect(deriveSchedulerSlug("Security Audit")).toBe("security-audit");
  });

  it("collapses runs of non-alphanumerics into one dash", () => {
    expect(deriveSchedulerSlug("GDPR — compliance & audit")).toBe("gdpr-compliance-audit");
  });

  it("strips leading and trailing dashes", () => {
    expect(deriveSchedulerSlug("  ...weekly review!  ")).toBe("weekly-review");
  });

  it("keeps digits", () => {
    expect(deriveSchedulerSlug("SOC2 Type 2")).toBe("soc2-type-2");
  });

  it("returns empty string when nothing usable remains", () => {
    expect(deriveSchedulerSlug("!!!")).toBe("");
    expect(deriveSchedulerSlug("")).toBe("");
  });
});

describe("diffSchedulerTemplate", () => {
  const saved = { name: "Audit", description: "Weekly", prompt: "Look", color: "#3b82f6" };

  it("returns nothing when the values match the saved template", () => {
    expect(diffSchedulerTemplate({ ...saved }, saved)).toEqual({});
  });

  it("returns only the fields that changed", () => {
    expect(diffSchedulerTemplate({ ...saved, prompt: "Look harder", description: "" }, saved)).toEqual({
      prompt: "Look harder",
      description: "",
    });
  });

  it("ignores color case", () => {
    expect(diffSchedulerTemplate({ ...saved, color: "#3B82F6" }, saved)).toEqual({});
    expect(diffSchedulerTemplate({ ...saved, color: "#ef4444" }, saved)).toEqual({ color: "#ef4444" });
  });
});
