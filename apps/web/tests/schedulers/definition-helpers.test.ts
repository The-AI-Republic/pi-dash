/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import { deriveSchedulerSlug } from "@/components/schedulers/definition-helpers";

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
