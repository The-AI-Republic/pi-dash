// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { INTAKE_AREA_LABEL, intakeDocumentTitle } from "./title.js";

describe("intakeDocumentTitle (INT-003)", () => {
  it("names the project plus the area", () => {
    expect(intakeDocumentTitle("Apollo")).toBe("Apollo - Intake");
    expect(INTAKE_AREA_LABEL).toBe("Intake");
  });

  it("falls back to the product name before the project is known", () => {
    expect(intakeDocumentTitle(undefined)).toBe("Pi Dash - Intake");
    expect(intakeDocumentTitle(null)).toBe("Pi Dash - Intake");
    expect(intakeDocumentTitle("   ")).toBe("Pi Dash - Intake");
  });
});
