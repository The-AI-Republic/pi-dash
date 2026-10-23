// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake edit permissions (INT-026): the full actor matrix plus the
// resolved-status rule.

import { INTAKE_STATUS } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { isResolvedIntakeStatus, selectIntakeEditability } from "./permissions.js";

describe("isResolvedIntakeStatus", () => {
  it.each([
    [INTAKE_STATUS.accepted, true],
    [INTAKE_STATUS.declined, true],
    [INTAKE_STATUS.duplicate, true],
    [INTAKE_STATUS.pending, false],
    [INTAKE_STATUS.snoozed, false],
  ])("status %i resolves to %s", (status, expected) => {
    expect(isResolvedIntakeStatus(status)).toBe(expected);
  });
});

describe("selectIntakeEditability", () => {
  it("lets an admin edit everything on an open request", () => {
    expect(
      selectIntakeEditability({ isAdmin: true, isCreator: false, isGuest: false, status: INTAKE_STATUS.pending })
    ).toEqual({ canEditContent: true, canEditProperties: true });
  });

  it("lets a member creator edit everything on their own request", () => {
    expect(
      selectIntakeEditability({ isAdmin: false, isCreator: true, isGuest: false, status: INTAKE_STATUS.pending })
    ).toEqual({ canEditContent: true, canEditProperties: true });
  });

  it("keeps a non-creator member read-only", () => {
    expect(
      selectIntakeEditability({ isAdmin: false, isCreator: false, isGuest: false, status: INTAKE_STATUS.pending })
    ).toEqual({ canEditContent: false, canEditProperties: false });
  });

  it("limits a guest creator to the content on their own request", () => {
    expect(
      selectIntakeEditability({ isAdmin: false, isCreator: true, isGuest: true, status: INTAKE_STATUS.pending })
    ).toEqual({ canEditContent: true, canEditProperties: false });
  });

  it("keeps a non-creator guest read-only", () => {
    expect(
      selectIntakeEditability({ isAdmin: false, isCreator: false, isGuest: true, status: INTAKE_STATUS.pending })
    ).toEqual({ canEditContent: false, canEditProperties: false });
  });

  it.each([INTAKE_STATUS.accepted, INTAKE_STATUS.declined, INTAKE_STATUS.duplicate])(
    "keeps a resolved request (%i) read-only even for its admin creator",
    (status) => {
      expect(selectIntakeEditability({ isAdmin: true, isCreator: true, isGuest: false, status })).toEqual({
        canEditContent: false,
        canEditProperties: false,
      });
    }
  );
});
