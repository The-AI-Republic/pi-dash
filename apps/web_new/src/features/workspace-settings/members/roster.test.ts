// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure roster rules (parity/specs/workspace-settings.md §7–§9, WSET-008/011).
import { describe, expect, it } from "vitest";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import {
  applyRosterFilters,
  authMediumLabel,
  columnOrderLabels,
  EMPTY_ROSTER_FILTERS,
  memberMatchesSearch,
  roleLabel,
  rosterCount,
  rosterFullName,
  rosterIdentityName,
  ROSTER_COLUMNS,
  ROSTER_ROLE_FILTERS,
  ROSTER_SORTABLE_COLUMNS,
  selectRosterMembers,
  toggleRoleFilter,
  type RosterFilters,
} from "./roster.js";

let nextId = 0;

function member(overrides: {
  first?: string;
  last?: string;
  display?: string;
  email?: string;
  role?: number;
  active?: boolean;
  bot?: boolean;
  created?: string;
  medium?: string;
  avatarUrl?: string | null;
}): WorkspaceRosterMember {
  nextId += 1;
  return {
    id: `123e4567-e89b-12d3-a456-4266141740${String(nextId).padStart(2, "0")}`,
    member: {
      id: `223e4567-e89b-12d3-a456-4266141740${String(nextId).padStart(2, "0")}`,
      first_name: overrides.first ?? "Ada",
      last_name: overrides.last ?? "Lovelace",
      avatar: "",
      avatar_url: overrides.avatarUrl ?? null,
      display_name: overrides.display ?? "Ada Lovelace",
      is_bot: overrides.bot ?? false,
      email: overrides.email ?? "ada@example.com",
      last_login_medium: overrides.medium ?? "email",
    },
    role: overrides.role ?? 15,
    is_active: overrides.active ?? true,
    created_at: overrides.created ?? "2026-01-01T00:00:00Z",
  };
}

function filters(overrides: Partial<RosterFilters> = {}): RosterFilters {
  return { ...EMPTY_ROSTER_FILTERS, ...overrides };
}

describe("roster selectors", () => {
  it("counts every non-bot membership, suspended included", () => {
    const rows = [member({}), member({ active: false }), member({ bot: true, email: "bot@example.com" })];
    expect(rosterCount(rows)).toBe(2);
    expect(selectRosterMembers(rows).map((row) => row.member.email)).toEqual(["ada@example.com", "ada@example.com"]);
  });

  it("matches search on first, last, display name and email, ignoring case", () => {
    const user = member({ first: "Grace", last: "Hopper", display: "Amazing Grace", email: "Grace.H@navy.mil" }).member;
    for (const needle of ["grace", "HOPPER", "amazing", "navy.mil", "  h@navy  "]) {
      expect(memberMatchesSearch(user, needle)).toBe(true);
    }
    expect(memberMatchesSearch(user, "turing")).toBe(false);
    expect(memberMatchesSearch(user, "")).toBe(true);
  });

  it("combines search with the role filter", () => {
    const rows = [
      member({ display: "Ada Admin", role: 20, email: "ada@example.com" }),
      member({ display: "Ada Member", role: 15, email: "ada.m@example.com" }),
      member({ display: "Bob Guest", role: 5, email: "bob@example.com" }),
    ];
    const visible = applyRosterFilters(rows, filters({ search: "ada", roles: ["member"] }));
    expect(visible.map((row) => row.member.display_name)).toEqual(["Ada Member"]);
  });

  it("treats Suspended as the deactivated rows and never mixes it with a role", () => {
    const rows = [
      member({ display: "Admin", role: 20 }),
      member({ display: "Gone Admin", role: 20, active: false }),
      member({ display: "Member", role: 15 }),
    ];
    expect(applyRosterFilters(rows, filters({ roles: ["suspended"] })).map((row) => row.member.display_name)).toEqual([
      "Gone Admin",
    ]);
    expect(applyRosterFilters(rows, filters({ roles: ["admin"] })).map((row) => row.member.display_name)).toEqual([
      "Admin",
    ]);
    expect(applyRosterFilters(rows, filters()).map((row) => row.member.display_name)).toEqual([
      "Admin",
      "Member",
      "Gone Admin",
    ]);
  });

  it("matches the three active roles by number", () => {
    const rows = [member({ role: 20 }), member({ role: 15 }), member({ role: 5 })];
    expect(applyRosterFilters(rows, filters({ roles: ["guest"] })).map((row) => row.role)).toEqual([5]);
    expect(applyRosterFilters(rows, filters({ roles: ["admin", "member"] })).map((row) => row.role)).toEqual([20, 15]);
  });

  it("sorts active rows above suspended ones while keeping server order inside each group", () => {
    const rows = [
      member({ display: "Suspended first", active: false }),
      member({ display: "Active one" }),
      member({ display: "Suspended second", active: false }),
      member({ display: "Active two" }),
    ];
    expect(applyRosterFilters(rows, filters()).map((row) => row.member.display_name)).toEqual([
      "Active one",
      "Active two",
      "Suspended first",
      "Suspended second",
    ]);
  });

  it("applies a chosen column order inside the active-first rule", () => {
    const rows = [
      member({ display: "Charlie", active: false, email: "c@example.com" }),
      member({ display: "alpha", email: "z@example.com" }),
      member({ display: "Bravo", email: "a@example.com" }),
    ];
    const asc = applyRosterFilters(rows, filters({ order: { column: "display_name", direction: "asc" } }));
    expect(asc.map((row) => row.member.display_name)).toEqual(["alpha", "Bravo", "Charlie"]);
    const desc = applyRosterFilters(rows, filters({ order: { column: "email", direction: "desc" } }));
    expect(desc.map((row) => row.member.email)).toEqual(["z@example.com", "a@example.com", "c@example.com"]);
  });

  it("orders by role and joining date, including unparseable dates", () => {
    const rows = [
      member({ display: "new", role: 5, created: "2026-03-01T00:00:00Z" }),
      member({ display: "old", role: 20, created: "2025-01-01T00:00:00Z" }),
      member({ display: "bad", role: 15, created: "not-a-date" }),
    ];
    expect(
      applyRosterFilters(rows, filters({ order: { column: "role", direction: "asc" } })).map((row) => row.role)
    ).toEqual([5, 15, 20]);
    expect(
      applyRosterFilters(rows, filters({ order: { column: "joining_date", direction: "asc" } })).map(
        (row) => row.member.display_name
      )
    ).toEqual(["bad", "old", "new"]);
  });

  it("treats either side of a date comparison as unparseable independently", () => {
    const rows = [
      member({ display: "good", created: "2025-01-01T00:00:00Z" }),
      member({ display: "bad-one", created: "not-a-date" }),
      member({ display: "bad-two", created: "" }),
    ];
    const ordered = applyRosterFilters(rows, filters({ order: { column: "joining_date", direction: "desc" } }));
    expect(ordered.map((row) => row.member.display_name)).toEqual(["good", "bad-one", "bad-two"]);
  });

  it("keeps full-name ordering case-insensitive", () => {
    const rows = [member({ first: "zoe", last: "Zulu" }), member({ first: "Amy", last: "Alpha" })];
    const ordered = applyRosterFilters(rows, filters({ order: { column: "full_name", direction: "asc" } }));
    expect(ordered.map((row) => rosterFullName(row.member))).toEqual(["Amy Alpha", "zoe Zulu"]);
  });
});

describe("roster labels", () => {
  it("maps stored sign-in media to their labels and hides unknown blanks", () => {
    expect(authMediumLabel("email")).toBe("Email");
    expect(authMediumLabel("magic-code")).toBe("Magic code");
    expect(authMediumLabel("github")).toBe("GitHub");
    expect(authMediumLabel("gitlab")).toBe("GitLab");
    expect(authMediumLabel("google")).toBe("Google");
    expect(authMediumLabel("gitea")).toBe("Gitea");
    // Unknown media are not named: the column hides instead of echoing a
    // raw provider code (§8).
    for (const medium of ["saml", "oidc", "Email"]) {
      expect(authMediumLabel(medium)).toBeNull();
    }
    expect(authMediumLabel(null)).toBeNull();
    expect(authMediumLabel(undefined)).toBeNull();
    expect(authMediumLabel("")).toBeNull();
  });

  it("labels numeric workspace roles", () => {
    expect(roleLabel(20)).toBe("Admin");
    expect(roleLabel(15)).toBe("Member");
    expect(roleLabel(5)).toBe("Guest");
    expect(roleLabel(1)).toBe("Guest");
  });

  it("falls back from display name to email, then to the joined name", () => {
    expect(rosterIdentityName(member({}).member)).toBe("Ada Lovelace");
    expect(rosterIdentityName(member({ display: "", email: "e@example.com" }).member)).toBe("e@example.com");
    expect(rosterIdentityName(member({ display: "", email: "", first: "Solo", last: "" }).member)).toBe("Solo");
    expect(rosterFullName(member({ first: "", last: "" }).member)).toBe("Ada Lovelace");
    expect(rosterFullName(member({ first: "Only" }).member)).toBe("Only Lovelace");
  });

  it("toggles role filters and empties back to no filter", () => {
    expect(toggleRoleFilter([], "admin")).toEqual(["admin"]);
    expect(toggleRoleFilter(["admin"], "guest")).toEqual(["admin", "guest"]);
    expect(toggleRoleFilter(["admin", "guest"], "admin")).toEqual(["guest"]);
  });

  it("offers the four filter labels in display order", () => {
    expect(ROSTER_ROLE_FILTERS.map((entry) => entry.label)).toEqual(["Admin", "Member", "Guest", "Suspended"]);
  });

  it("pins the six table columns and five sortable headers", () => {
    expect(ROSTER_COLUMNS.map((column) => column.label)).toEqual([
      "Full name",
      "Display name",
      "Email address",
      "Account type",
      "Authentication",
      "Joining date",
    ]);
    expect(ROSTER_SORTABLE_COLUMNS.map((column) => column.key)).toEqual([
      "full_name",
      "display_name",
      "email",
      "role",
      "joining_date",
    ]);
  });

  it("gives each header its own direction wording", () => {
    expect(columnOrderLabels({ key: "full_name", label: "Full name" })).toEqual({ asc: "A → Z", desc: "Z → A" });
    expect(columnOrderLabels({ key: "role", label: "Account type" })).toEqual({
      asc: "Guest → Admin",
      desc: "Admin → Guest",
    });
    expect(columnOrderLabels({ key: "joining_date", label: "Joining date" })).toEqual({
      asc: "Oldest → Newest",
      desc: "Newest → Oldest",
    });
  });
});
