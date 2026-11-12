// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { WorkspaceMemberList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { INTAKE_IDENTITY_EMAIL, isIntakeIdentityCreator } from "./identity.js";

function entry(id: string, email: string | undefined) {
  return {
    id: `membership-${id}`,
    member: {
      id,
      first_name: "Intake",
      last_name: "User",
      avatar: "",
      avatar_url: null,
      display_name: "Intake User",
      is_bot: false,
      ...(email === undefined ? {} : { email }),
    },
    role: 15,
    is_active: true,
  };
}

const DIRECTORY = [entry("user-system", INTAKE_IDENTITY_EMAIL), entry("user-member", "member@example.com")];

describe("intake identity (INT-006)", () => {
  it("names the parity seed's intake address", () => {
    expect(INTAKE_IDENTITY_EMAIL).toBe("intake@example.com");
  });

  it("matches the creator behind the intake address", () => {
    expect(isIntakeIdentityCreator(DIRECTORY, "user-system")).toBe(true);
    expect(isIntakeIdentityCreator(DIRECTORY, "user-member")).toBe(false);
  });

  it("never matches a missing creator, directory or email", () => {
    expect(isIntakeIdentityCreator(DIRECTORY, null)).toBe(false);
    expect(isIntakeIdentityCreator(undefined, "user-system")).toBe(false);
    expect(isIntakeIdentityCreator(DIRECTORY, "user-unknown")).toBe(false);
    const guestLens: WorkspaceMemberList = [entry("user-system", undefined)];
    expect(isIntakeIdentityCreator(guestLens, "user-system")).toBe(false);
  });
});
