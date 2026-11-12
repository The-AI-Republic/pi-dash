// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure roster rules (PSET-018–021): join, bot filter, search, role filter,
// per-column order, tie stability and the cleared default.
import type { ProjectMember, WorkspaceMember } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  applyMemberRosterFilters,
  buildMemberRoster,
  EMPTY_MEMBER_ROSTER_FILTERS,
  formatJoiningDate,
  memberMatchesRoleFilter,
  memberMatchesSearch,
  memberRoleKey,
  memberRoleLabel,
  toggleMemberRole,
  type MemberRosterRow,
  type MemberSort,
} from "./roster.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

const USERS = {
  ada: "123e4567-e89b-12d3-a456-42661417400a",
  grace: "123e4567-e89b-12d3-a456-42661417400b",
  linus: "123e4567-e89b-12d3-a456-42661417400c",
  bot: "123e4567-e89b-12d3-a456-42661417400d",
  ghost: "123e4567-e89b-12d3-a456-42661417400e",
} as const;

function directoryRow(
  id: string,
  fields: {
    first?: string;
    last?: string;
    display?: string;
    email?: string | undefined;
    avatar?: string | null;
    bot?: boolean;
  }
): WorkspaceMember {
  return {
    id: `${id}-membership`,
    role: 15,
    member: {
      id,
      first_name: fields.first ?? "",
      last_name: fields.last ?? "",
      avatar: "",
      avatar_url: fields.avatar ?? null,
      display_name: fields.display ?? "",
      is_bot: fields.bot ?? false,
      ...(fields.email === undefined ? {} : { email: fields.email }),
    },
  };
}

function membership(id: string, userId: string, role: number, joinedAt: string): ProjectMember {
  return {
    id,
    role,
    member: userId,
    project: PROJECT,
    original_role: role,
    created_at: joinedAt,
  };
}

const DIRECTORY: WorkspaceMember[] = [
  directoryRow(USERS.ada, {
    first: "Ada",
    last: "Lovelace",
    display: "ada",
    email: "ada@example.test",
    avatar: "/avatar/ada.png",
  }),
  directoryRow(USERS.grace, { first: "Grace", last: "Hopper", display: "Grace H", email: "grace@example.test" }),
  directoryRow(USERS.linus, { first: "Linus", last: "Torvalds", display: "linus" }),
  directoryRow(USERS.bot, { first: "Build", last: "Bot", display: "build-bot", bot: true }),
];

const MEMBERSHIPS: ProjectMember[] = [
  membership("m-ada", USERS.ada, 20, "2025-01-10T09:00:00Z"),
  membership("m-grace", USERS.grace, 15, "2025-03-02T09:00:00Z"),
  membership("m-linus", USERS.linus, 5, ""),
  membership("m-bot", USERS.bot, 15, "2025-02-01T09:00:00Z"),
];

function rows(): MemberRosterRow[] {
  return buildMemberRoster(MEMBERSHIPS, DIRECTORY);
}

function names(list: readonly MemberRosterRow[]): string[] {
  return list.map((row) => row.fullName);
}

function displays(list: readonly MemberRosterRow[]): string[] {
  return list.map((row) => row.displayName);
}

describe("buildMemberRoster", () => {
  it("joins memberships to the directory and drops bot memberships", () => {
    const roster = rows();
    expect(roster.map((row) => row.membershipId)).toEqual(["m-ada", "m-grace", "m-linus"]);
    expect(roster[0]).toMatchObject({
      userId: USERS.ada,
      firstName: "Ada",
      lastName: "Lovelace",
      fullName: "Ada Lovelace",
      displayName: "ada",
      email: "ada@example.test",
      avatarUrl: "/avatar/ada.png",
      role: 20,
      roleLabel: "Admin",
      joinedAt: "2025-01-10T09:00:00Z",
    });
  });

  it("leaves the email blank when the directory lens omits it", () => {
    expect(rows()[2]?.email).toBe("");
  });

  it("renders a membership the directory does not know rather than dropping it", () => {
    const roster = buildMemberRoster([membership("m-ghost", USERS.ghost, 5, "")], DIRECTORY);
    expect(roster).toHaveLength(1);
    expect(roster[0]).toMatchObject({
      userId: USERS.ghost,
      fullName: "",
      displayName: "",
      email: "",
      roleLabel: "Guest",
    });
  });

  it("falls back to the display name when no full name is set, and back again", () => {
    const onlyDisplay = buildMemberRoster(
      [membership("m-x", USERS.ada, 15, "2025-01-01T00:00:00Z")],
      [directoryRow(USERS.ada, { display: "only-display" })]
    );
    expect(onlyDisplay[0]).toMatchObject({ fullName: "only-display", displayName: "only-display" });
    const onlyName = buildMemberRoster(
      [membership("m-y", USERS.grace, 15, "2025-01-01T00:00:00Z")],
      [directoryRow(USERS.grace, { first: "Grace", last: "Hopper" })]
    );
    expect(onlyName[0]).toMatchObject({ fullName: "Grace Hopper", displayName: "Grace Hopper" });
  });
});

describe("member search", () => {
  it("matches display and full names case-insensitively", () => {
    const roster = rows();
    expect(names(roster.filter((row) => memberMatchesSearch(row, "LOVE")))).toEqual(["Ada Lovelace"]);
    expect(names(roster.filter((row) => memberMatchesSearch(row, "grace h")))).toEqual(["Grace Hopper"]);
    expect(names(roster.filter((row) => memberMatchesSearch(row, "ada")))).toEqual(["Ada Lovelace"]);
    expect(names(roster.filter((row) => memberMatchesSearch(row, "  lin  ")))).toEqual(["Linus Torvalds"]);
  });

  it("does not search the email", () => {
    expect(rows().filter((row) => memberMatchesSearch(row, "example.test"))).toEqual([]);
  });

  it("matches full names when the directory gave no display name", () => {
    const roster = buildMemberRoster(
      [membership("m-ada", USERS.ada, 15, "2025-01-01T00:00:00Z")],
      [directoryRow(USERS.ada, { first: "Ada", last: "Lovelace" })]
    );
    expect(roster[0]?.rawDisplayName).toBe("");
    expect(roster.filter((row) => memberMatchesSearch(row, "love")).map((row) => row.fullName)).toEqual([
      "Ada Lovelace",
    ]);
  });

  it("admits everything for a blank query", () => {
    expect(rows().filter((row) => memberMatchesSearch(row, "   "))).toHaveLength(3);
  });
});

describe("role labels and filter", () => {
  it("maps stored roles to words and filter keys", () => {
    expect([20, 15, 5].map(memberRoleLabel)).toEqual(["Admin", "Member", "Guest"]);
    expect([20, 15, 5].map(memberRoleKey)).toEqual(["admin", "member", "guest"]);
    expect(memberRoleLabel(30)).toBe("Admin");
    expect(memberRoleKey(0)).toBe("guest");
  });

  it("admits every row with no checked role and narrows to the checked ones", () => {
    const roster = rows();
    expect(roster.filter((row) => memberMatchesRoleFilter(row, []))).toHaveLength(3);
    expect(names(roster.filter((row) => memberMatchesRoleFilter(row, ["admin"])))).toEqual(["Ada Lovelace"]);
    expect(names(roster.filter((row) => memberMatchesRoleFilter(row, ["member", "guest"])))).toEqual([
      "Grace Hopper",
      "Linus Torvalds",
    ]);
  });

  it("toggles a role on and off again", () => {
    expect(toggleMemberRole([], "member")).toEqual(["member"]);
    expect(toggleMemberRole(["member"], "admin")).toEqual(["member", "admin"]);
    expect(toggleMemberRole(["member", "admin"], "member")).toEqual(["admin"]);
  });
});

describe("applyMemberRosterFilters", () => {
  it("keeps the server order when nothing is applied", () => {
    expect(names(applyMemberRosterFilters(rows(), EMPTY_MEMBER_ROSTER_FILTERS))).toEqual([
      "Ada Lovelace",
      "Grace Hopper",
      "Linus Torvalds",
    ]);
  });

  it("applies search and role filter before ordering", () => {
    const visible = applyMemberRosterFilters(rows(), {
      search: "o",
      roles: ["member", "guest"],
      sort: { column: "display_name", direction: "desc" },
    });
    expect(names(visible)).toEqual(["Linus Torvalds", "Grace Hopper"]);
  });

  it("orders each column in both directions", () => {
    const ascending = (column: MemberSort) =>
      names(applyMemberRosterFilters(rows(), { search: "", roles: [], sort: column }));
    expect(ascending({ column: "full_name", direction: "asc" })).toEqual([
      "Ada Lovelace",
      "Grace Hopper",
      "Linus Torvalds",
    ]);
    expect(ascending({ column: "full_name", direction: "desc" })).toEqual([
      "Linus Torvalds",
      "Grace Hopper",
      "Ada Lovelace",
    ]);
    const byDisplay = (direction: "asc" | "desc"): string[] =>
      displays(
        applyMemberRosterFilters(rows(), { search: "", roles: [], sort: { column: "display_name", direction } })
      );
    expect(byDisplay("asc")).toEqual(["ada", "Grace H", "linus"]);
    expect(byDisplay("desc")).toEqual(["linus", "Grace H", "ada"]);
    expect(ascending({ column: "email", direction: "asc" })).toEqual([
      "Linus Torvalds",
      "Ada Lovelace",
      "Grace Hopper",
    ]);
    expect(ascending({ column: "role", direction: "asc" })).toEqual(["Ada Lovelace", "Linus Torvalds", "Grace Hopper"]);
    expect(ascending({ column: "role", direction: "desc" })).toEqual([
      "Grace Hopper",
      "Linus Torvalds",
      "Ada Lovelace",
    ]);
  });

  it("sorts a missing joining date as the oldest value", () => {
    const byDate = (direction: "asc" | "desc"): string[] =>
      names(applyMemberRosterFilters(rows(), { search: "", roles: [], sort: { column: "joining_date", direction } }));
    expect(byDate("asc")).toEqual(["Linus Torvalds", "Ada Lovelace", "Grace Hopper"]);
    expect(byDate("desc")).toEqual(["Grace Hopper", "Ada Lovelace", "Linus Torvalds"]);
  });

  it("keeps ties in their incoming order", () => {
    const tied = buildMemberRoster(
      [
        membership("m-1", USERS.ada, 15, "2025-01-01T00:00:00Z"),
        membership("m-2", USERS.grace, 15, "2025-01-01T00:00:00Z"),
      ],
      [
        directoryRow(USERS.ada, { first: "Same", last: "Name" }),
        directoryRow(USERS.grace, { first: "Same", last: "Name" }),
      ]
    );
    expect(
      applyMemberRosterFilters(tied, { search: "", roles: [], sort: { column: "full_name", direction: "asc" } }).map(
        (row) => row.membershipId
      )
    ).toEqual(["m-1", "m-2"]);
  });

  it("sorts an unparseable joining date as the oldest value too", () => {
    const mixed = buildMemberRoster(
      [membership("m-1", USERS.ada, 15, "not-a-date"), membership("m-2", USERS.grace, 15, "2025-03-02T09:00:00Z")],
      [
        directoryRow(USERS.ada, { first: "Ada", last: "Lovelace" }),
        directoryRow(USERS.grace, { first: "Grace", last: "Hopper" }),
      ]
    );
    const byDate = (direction: "asc" | "desc"): string[] =>
      names(applyMemberRosterFilters(mixed, { search: "", roles: [], sort: { column: "joining_date", direction } }));
    expect(byDate("asc")).toEqual(["Ada Lovelace", "Grace Hopper"]);
    expect(byDate("desc")).toEqual(["Grace Hopper", "Ada Lovelace"]);
  });

  it("returns every row once the filters and sort clear", () => {
    expect(applyMemberRosterFilters(rows(), { search: "", roles: [], sort: null })).toHaveLength(3);
  });
});

describe("formatJoiningDate", () => {
  it("formats a parseable timestamp and blanks an unparseable one", () => {
    expect(formatJoiningDate("2025-03-02T09:00:00Z")).toBe("Mar 2, 2025");
    expect(formatJoiningDate("")).toBe("");
    expect(formatJoiningDate("not-a-date")).toBe("");
  });
});
