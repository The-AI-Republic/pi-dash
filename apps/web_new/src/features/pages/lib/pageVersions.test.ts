// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Version timeline model (parity/specs/pages.md §6.9, PAGE-051): the live
// row leads and is active only when no snapshot is open, snapshots sort
// newest-first with their saved-at text and author, and an unanswered
// directory never invents an author.

import type { PageVersion } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { PAGE_DEACTIVATED_USER, type PageActorMember } from "./pagePane.js";
import { PAGE_CURRENT_VERSION_LABEL, pageVersionEntries, pageVersionTime } from "./pageVersions.js";

const OWNER = "11111111-1111-4111-8111-111111111111";
const MEMBER = "22222222-2222-4222-8222-222222222222";

function version(id: string, ownedBy: string, savedAt: string): PageVersion {
  return {
    id,
    workspace: "33333333-3333-4333-8333-333333333333",
    page: "44444444-4444-4444-8444-444444444444",
    last_saved_at: savedAt,
    owned_by: ownedBy,
    created_at: savedAt,
    updated_at: savedAt,
    created_by: null,
    updated_by: null,
  };
}

const MEMBERS: PageActorMember[] = [
  { id: OWNER, name: "Owner Name", avatarUrl: null },
  { id: MEMBER, name: "Member Name", avatarUrl: null },
];

const OLDER = version("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", OWNER, "2026-10-20T10:00:00Z");
const NEWER = version("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", MEMBER, "2026-10-21T10:00:00Z");

describe("pageVersionTime", () => {
  it("renders a saved-at moment with the year", () => {
    expect(pageVersionTime("2026-10-21T10:00:00Z")).toMatch(/2026/);
  });

  it("passes an unreadable value through unchanged", () => {
    expect(pageVersionTime("not a date")).toBe("not a date");
  });
});

describe("pageVersionEntries", () => {
  it("leads with the live row while the history is still loading", () => {
    expect(pageVersionEntries({ versions: undefined, activeVersionId: undefined, members: MEMBERS })).toEqual([
      { id: null, label: PAGE_CURRENT_VERSION_LABEL, author: "", active: true },
    ]);
  });

  it("lists the live row first, then the snapshots newest-first", () => {
    const entries = pageVersionEntries({ versions: [OLDER, NEWER], activeVersionId: undefined, members: MEMBERS });
    expect(entries.map((entry) => entry.id)).toEqual([null, NEWER.id, OLDER.id]);
    expect(entries[0]?.label).toBe(PAGE_CURRENT_VERSION_LABEL);
  });

  it("names each snapshot's author from the directory", () => {
    const entries = pageVersionEntries({ versions: [OLDER, NEWER], activeVersionId: undefined, members: MEMBERS });
    expect(entries[1]?.author).toBe("Member Name");
    expect(entries[2]?.author).toBe("Owner Name");
    expect(entries[0]?.author).toBe("");
  });

  it("leaves the author blank until the directory answers", () => {
    const entries = pageVersionEntries({ versions: [OLDER], activeVersionId: undefined, members: undefined });
    expect(entries[1]?.author).toBe("");
  });

  it("marks a departed member with the neutral label once the directory knows", () => {
    const entries = pageVersionEntries({ versions: [OLDER], activeVersionId: undefined, members: [MEMBERS[1]!] });
    expect(entries[1]?.author).toBe(PAGE_DEACTIVATED_USER);
  });

  it("keeps the live row active while no snapshot is open", () => {
    const entries = pageVersionEntries({ versions: [OLDER, NEWER], activeVersionId: undefined, members: MEMBERS });
    expect(entries.filter((entry) => entry.active).map((entry) => entry.id)).toEqual([null]);
  });

  it("makes exactly the addressed snapshot active and the live row idle", () => {
    const entries = pageVersionEntries({ versions: [OLDER, NEWER], activeVersionId: NEWER.id, members: MEMBERS });
    expect(entries.filter((entry) => entry.active).map((entry) => entry.id)).toEqual([NEWER.id]);
  });

  it("leaves every row idle when the address names a snapshot that is gone", () => {
    const entries = pageVersionEntries({ versions: [OLDER], activeVersionId: NEWER.id, members: MEMBERS });
    expect(entries.some((entry) => entry.active)).toBe(false);
  });

  it("sorts an unreadable timestamp last instead of scrambling the order", () => {
    const broken = version("cccccccc-cccc-4ccc-8ccc-cccccccccccc", OWNER, "not a date");
    const entries = pageVersionEntries({ versions: [broken, OLDER], activeVersionId: undefined, members: MEMBERS });
    expect(entries.map((entry) => entry.id)).toEqual([null, OLDER.id, broken.id]);
  });
});
