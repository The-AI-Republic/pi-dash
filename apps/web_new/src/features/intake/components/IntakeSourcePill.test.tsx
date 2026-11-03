// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const editionHarness = vi.hoisted(() => ({ flags: {} as Record<string, boolean> }));
vi.mock("../../../core/edition/index.js", () => ({
  INTAKE_SOURCE_PILL_FLAG: "intake-source-pill",
  get edition() {
    return { id: "test", flags: editionHarness.flags };
  },
}));

import { INTAKE_SOURCE_PILL_FLAG } from "../../../core/edition/index.js";
import { INTAKE_IN_APP_SOURCE, IntakeSourcePill, intakeSourcePillLabel } from "./IntakeSourcePill.js";

afterEach(() => {
  editionHarness.flags = {};
});

describe("intakeSourcePillLabel", () => {
  it("hides the bare default and the missing origin", () => {
    expect(intakeSourcePillLabel(null)).toBeNull();
    expect(intakeSourcePillLabel(INTAKE_IN_APP_SOURCE)).toBeNull();
    expect(intakeSourcePillLabel("")).toBeNull();
    expect(intakeSourcePillLabel("___")).toBeNull();
  });

  it("humanizes cloud origins generically", () => {
    expect(intakeSourcePillLabel("EMAIL")).toBe("Email");
    expect(intakeSourcePillLabel("FORMS")).toBe("Forms");
    expect(intakeSourcePillLabel("CONTACT_FORM")).toBe("Contact Form");
  });
});

describe("IntakeSourcePill", () => {
  it("renders nothing while the edition leaves the flag absent (OSS)", () => {
    render(<IntakeSourcePill source="EMAIL" />);
    expect(screen.queryByTestId("intake-source-pill")).toBeNull();
  });

  it("renders nothing for the in-app default even with the flag on", () => {
    editionHarness.flags = { [INTAKE_SOURCE_PILL_FLAG]: true };
    const { rerender } = render(<IntakeSourcePill source={INTAKE_IN_APP_SOURCE} />);
    expect(screen.queryByTestId("intake-source-pill")).toBeNull();
    rerender(<IntakeSourcePill source={null} />);
    expect(screen.queryByTestId("intake-source-pill")).toBeNull();
  });

  it("renders the humanized pill with the flag on and a cloud origin", () => {
    editionHarness.flags = { [INTAKE_SOURCE_PILL_FLAG]: true };
    render(<IntakeSourcePill source="CONTACT_FORM" />);
    const pill = screen.getByTestId("intake-source-pill");
    expect(pill).toHaveTextContent("Contact Form");
    expect(pill).toHaveAttribute("data-source", "CONTACT_FORM");
  });
});
