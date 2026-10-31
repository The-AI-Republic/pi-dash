// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { catalogFailureMessage, writeFailureMessage } from "./lib/errors.js";

describe("catalog failure message", () => {
  it("repeats the server detail when it sent one (CLOUD-030)", () => {
    const error = new ApiError({ status: 500, code: "http", message: "Status check blew up." });
    expect(catalogFailureMessage(error)).toBe("Status check blew up.");
  });

  it("names the unreachable service when nothing answered (CLOUD-030)", () => {
    const error = new ApiError({ status: 0, code: "aborted", message: "Request was aborted" });
    expect(catalogFailureMessage(error)).toContain("could not be reached");
  });

  it("falls back generically off-protocol", () => {
    expect(catalogFailureMessage(new Error("boom"))).toContain("did not load");
    expect(catalogFailureMessage("boom")).toContain("did not load");
  });
});

describe("write failure message", () => {
  it("repeats the server detail for refused writes (CLOUD-024/027)", () => {
    const error = new ApiError({ status: 502, code: "http", message: "Hub refused the install." });
    expect(writeFailureMessage(error)).toBe("Hub refused the install.");
  });

  it("falls back generically off-protocol", () => {
    expect(writeFailureMessage(new Error("boom"))).toContain("went wrong");
    expect(writeFailureMessage("boom")).toContain("went wrong");
  });
});
