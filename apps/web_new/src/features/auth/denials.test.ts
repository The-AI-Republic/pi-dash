// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { decideProjectDenial, decideWorkspaceDenial, isProjectGateError } from "./denials.js";

describe("project denial decider", () => {
  it("offers joining to non-members", () => {
    expect(decideProjectDenial({ status: 409, isAdmin: false })).toBe("join");
  });

  it("offers joining to admins facing a private project", () => {
    expect(decideProjectDenial({ status: 403, isAdmin: true })).toBe("join");
  });

  it("explains without a join action to refused non-admins", () => {
    expect(decideProjectDenial({ status: 403, isAdmin: false })).toBe("no-access");
  });

  it("reports not-found for unknown or otherwise failed projects", () => {
    expect(decideProjectDenial({ status: 404, isAdmin: false })).toBe("not-found");
    expect(decideProjectDenial({ status: 500, isAdmin: true })).toBe("not-found");
    expect(decideProjectDenial({ status: undefined, isAdmin: false })).toBe("not-found");
  });
});

describe("workspace denial decider", () => {
  it("reports not-found for unknown slugs", () => {
    expect(decideWorkspaceDenial({ known: false, membershipPresent: false })).toBe("not-found");
  });

  it("renders the not-a-member screen for known slugs without membership", () => {
    expect(decideWorkspaceDenial({ known: true, membershipPresent: false })).toBe("not-member");
  });

  it("renders children for members", () => {
    expect(decideWorkspaceDenial({ known: true, membershipPresent: true })).toBeNull();
  });
});

describe("project gate errors", () => {
  it("owns denial statuses", () => {
    for (const status of [403, 404, 409]) {
      expect(isProjectGateError(new ApiError({ status, code: "http", message: "denied" }))).toBe(true);
    }
  });

  it("lets anything else throw", () => {
    expect(isProjectGateError(new ApiError({ status: 500, code: "http", message: "broke" }))).toBe(false);
    expect(isProjectGateError(new ApiError({ status: 0, code: "network", message: "offline" }))).toBe(false);
    expect(isProjectGateError(new Error("plain"))).toBe(false);
    expect(isProjectGateError(null)).toBe(false);
  });
});
