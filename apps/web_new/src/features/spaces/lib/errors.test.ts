// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { classifySpaceFailure, spaceErrorStatus } from "./errors.js";

describe("space failure classification", () => {
  it("treats only an explicit 404 as anchor-not-found", () => {
    expect(classifySpaceFailure(new ApiError({ status: 404, code: "http", message: "missing" }))).toBe("not-found");
    expect(classifySpaceFailure(new ApiError({ status: 500, code: "http", message: "down" }))).toBe("error");
    expect(classifySpaceFailure(new ApiError({ code: "network", message: "offline" }))).toBe("error");
    expect(classifySpaceFailure(new Error("plain"))).toBe("error");
    expect(classifySpaceFailure("nope")).toBe("error");
  });

  it("reports the HTTP status when one exists", () => {
    expect(spaceErrorStatus(new ApiError({ status: 503, code: "http", message: "down" }))).toBe(503);
    expect(spaceErrorStatus(new Error("plain"))).toBeNull();
  });
});
