// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request status chip (INT-022/024): resolved names, Pending, the
// snooze countdown, and the lapsed passed tone.

import { render, screen } from "@testing-library/react";
import { INTAKE_STATUS } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { IntakeStatusChip } from "./IntakeStatusChip.js";

/** Local-midnight ISO timestamp a whole number of days from today. */
function stamp(daysFromToday: number): string {
  const at = new Date();
  at.setDate(at.getDate() + daysFromToday);
  at.setHours(0, 0, 0, 0);
  return at.toISOString();
}

function chip(status: number, snoozedTill: string | null = null): { text: string | null; tone: string | null } {
  const view = render(<IntakeStatusChip status={status} snoozedTill={snoozedTill} />);
  const node = screen.getByTestId("intake-status-chip");
  const found = { text: node.textContent, tone: node.getAttribute("data-tone") };
  view.unmount();
  return found;
}

describe("IntakeStatusChip", () => {
  it("names resolved and pending requests with the default tone", () => {
    expect(chip(INTAKE_STATUS.accepted).text).toBe("Accepted");
    expect(chip(INTAKE_STATUS.declined).text).toBe("Declined");
    expect(chip(INTAKE_STATUS.duplicate).text).toBe("Duplicate");
    expect(chip(INTAKE_STATUS.pending)).toEqual({ text: "Pending", tone: "default" });
  });

  it("counts whole days to the snooze day", () => {
    expect(chip(INTAKE_STATUS.snoozed, stamp(7)).text).toBe("7 days to go");
    expect(chip(INTAKE_STATUS.snoozed, stamp(1)).text).toBe("1 day to go");
    expect(chip(INTAKE_STATUS.snoozed, stamp(0)).text).toBe("Due today");
  });

  it("keeps a lapsed snooze visible with the passed tone", () => {
    expect(chip(INTAKE_STATUS.snoozed, stamp(-1))).toEqual({ text: "Overdue", tone: "danger" });
  });
});
