// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { INTAKE_STATUS, type IntakeStatus } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { INTAKE_STATUS_LABELS, intakeStatusChipLabel, orderStatusesForTab, statusOptionsForTab } from "./statuses.js";

describe("intake statuses (INT-006/009)", () => {
  it("labels every known status", () => {
    expect(INTAKE_STATUS_LABELS).toEqual({
      [-2]: "Pending",
      [-1]: "Declined",
      [0]: "Snoozed",
      [1]: "Accepted",
      [2]: "Duplicate",
    });
  });

  it("hides the chip while pending and for unknown statuses", () => {
    expect(intakeStatusChipLabel(INTAKE_STATUS.pending)).toBe(null);
    expect(intakeStatusChipLabel(99)).toBe(null);
  });

  it("names the chip for resolved and snoozed rows", () => {
    expect(intakeStatusChipLabel(INTAKE_STATUS.declined)).toBe("Declined");
    expect(intakeStatusChipLabel(INTAKE_STATUS.snoozed)).toBe("Snoozed");
    expect(intakeStatusChipLabel(INTAKE_STATUS.accepted)).toBe("Accepted");
    expect(intakeStatusChipLabel(INTAKE_STATUS.duplicate)).toBe("Duplicate");
  });

  it("offers pending and snoozed on Open, the resolved three on Closed", () => {
    expect(statusOptionsForTab("open")).toEqual([INTAKE_STATUS.pending, INTAKE_STATUS.snoozed]);
    expect(statusOptionsForTab("closed")).toEqual([
      INTAKE_STATUS.accepted,
      INTAKE_STATUS.declined,
      INTAKE_STATUS.duplicate,
    ]);
  });

  it("orders a selection by the tab's option order", () => {
    expect(orderStatusesForTab("closed", [INTAKE_STATUS.duplicate, INTAKE_STATUS.accepted])).toEqual([
      INTAKE_STATUS.accepted,
      INTAKE_STATUS.duplicate,
    ]);
    expect(orderStatusesForTab("open", [INTAKE_STATUS.snoozed, INTAKE_STATUS.pending])).toEqual([
      INTAKE_STATUS.pending,
      INTAKE_STATUS.snoozed,
    ]);
  });

  it("keeps unknown values at the end instead of dropping them", () => {
    expect(orderStatusesForTab("open", [99 as IntakeStatus, 100 as IntakeStatus, INTAKE_STATUS.pending])).toEqual([
      INTAKE_STATUS.pending,
      99,
      100,
    ]);
  });
});
