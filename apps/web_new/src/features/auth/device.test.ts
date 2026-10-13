// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  describeDeviceApprovalFailure,
  DEVICE_APPROVE_FAILED_MESSAGE,
  formatDeviceCode,
  isCompleteDeviceCode,
  normalizeDeviceCode,
} from "./device.js";

describe("device code formatting", () => {
  it("strips noise and uppercases", () => {
    expect(normalizeDeviceCode("ab-12 cd!")).toBe("AB12CD");
  });

  it("groups eight characters with a dash", () => {
    expect(formatDeviceCode("abcd1234")).toBe("ABCD-1234");
    expect(formatDeviceCode("ABCD-1234")).toBe("ABCD-1234");
    expect(formatDeviceCode("ab12")).toBe("AB12");
    expect(formatDeviceCode("a")).toBe("A");
  });

  it("caps the field at eight code characters", () => {
    expect(formatDeviceCode("abcd1234extra")).toBe("ABCD-1234");
  });

  it("completes on eight code characters with or without the dash", () => {
    expect(isCompleteDeviceCode("ABCD-1234")).toBe(true);
    expect(isCompleteDeviceCode("abcd1234")).toBe(true);
    expect(isCompleteDeviceCode("ABCD-123")).toBe(false);
    expect(isCompleteDeviceCode("")).toBe(false);
  });
});

describe("device approval failures", () => {
  it("prefers the server detail", () => {
    expect(describeDeviceApprovalFailure(new Error("Code not recognized."))).toBe("Code not recognized.");
  });

  it("falls back to the retry line without one", () => {
    expect(describeDeviceApprovalFailure(new Error(""))).toBe(DEVICE_APPROVE_FAILED_MESSAGE);
    expect(describeDeviceApprovalFailure(null)).toBe(DEVICE_APPROVE_FAILED_MESSAGE);
  });
});
