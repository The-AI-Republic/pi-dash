// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { NotificationListResponse } from "@pidash/api-client";

import { applyBulkRead, isRowInBulkReadScope, markPageRead, type CachedListPage } from "./bulkRead.js";

const READ_AT = "2026-10-17T12:00:00.000Z";

function page(read: (readAt: string | null) => string | null = (readAt) => readAt): NotificationListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 2,
    next_cursor: "300:0:0",
    prev_cursor: "300:-1:0",
    next_page_results: false,
    prev_page_results: false,
    count: 2,
    total_results: 2,
    results: [
      {
        id: "523e4567-e89b-12d3-a456-426614174004",
        workspace: "323e4567-e89b-12d3-a456-426614174002",
        project: null,
        data: null,
        entity_identifier: null,
        entity_name: "issue",
        title: "",
        message: null,
        message_html: "",
        message_stripped: null,
        sender: "in_app:issue_activities:default",
        triggered_by: null,
        triggered_by_details: null,
        receiver: "723e4567-e89b-12d3-a456-426614174006",
        read_at: read(null),
        snoozed_till: null,
        archived_at: null,
        created_at: "2026-10-16T12:00:00Z",
        updated_at: "2026-10-16T12:00:00Z",
        created_by: null,
        updated_by: null,
      },
    ],
  };
}

describe("bulk read scope (NTF-014)", () => {
  it("matches the same origin and exclusion facets", () => {
    expect(isRowInBulkReadScope({ snoozed: false, archived: false }, { snoozed: false, archived: false })).toBe(true);
    expect(
      isRowInBulkReadScope(
        { type: "assigned,created", snoozed: true, archived: false },
        { type: "assigned,created", snoozed: true, archived: false }
      )
    ).toBe(true);
  });

  it("ignores the read flag the unread-only mode adds to the stream", () => {
    expect(
      isRowInBulkReadScope({ snoozed: true, archived: false, read: false }, { snoozed: true, archived: false })
    ).toBe(true);
  });

  it("ignores the mentioned flag of the other tab", () => {
    expect(
      isRowInBulkReadScope({ snoozed: false, archived: false, mentioned: true }, { snoozed: false, archived: false })
    ).toBe(true);
  });

  it("rejects a different origin or exclusion facet", () => {
    expect(
      isRowInBulkReadScope({ type: "assigned", snoozed: false, archived: false }, { snoozed: false, archived: false })
    ).toBe(false);
    expect(isRowInBulkReadScope({ snoozed: false, archived: true }, { snoozed: false, archived: false })).toBe(false);
    expect(isRowInBulkReadScope({ snoozed: true, archived: false }, { snoozed: false, archived: false })).toBe(false);
  });
});

describe("mark page read", () => {
  it("stamps every unread row and keeps already-read rows' timestamps", () => {
    const marked = markPageRead(page(), READ_AT);
    expect(marked?.results[0]?.read_at).toBe(READ_AT);
    const already = page(() => "2026-10-16T09:00:00.000Z");
    expect(markPageRead(already, READ_AT)?.results[0]?.read_at).toBe("2026-10-16T09:00:00.000Z");
  });

  it("returns a new page and leaves the input untouched", () => {
    const original = page();
    const marked = markPageRead(original, READ_AT);
    expect(marked).not.toBe(original);
    expect(marked?.results[0]).not.toBe(original.results[0]);
    expect(original.results[0]?.read_at).toBeNull();
    expect(marked?.total_count).toBe(original.total_count);
  });
});

describe("apply bulk read", () => {
  const base = ["ws", "acme", "notifications", "list"] as const;

  function entry(
    query: CachedListPage["query"],
    key: readonly unknown[] = [null, false, false, null, false]
  ): CachedListPage {
    return { key: [...base, ...key], query, page: page() };
  }

  it("patches only the pages inside the posted scope", () => {
    const inScope = entry({ snoozed: false, archived: false }, [null, false, false, null, false]);
    const unreadOnly = entry({ snoozed: false, archived: false, read: false }, [null, false, false, false, false]);
    const otherOrigin = entry({ type: "assigned", snoozed: false, archived: false }, [
      "assigned",
      false,
      false,
      null,
      false,
    ]);
    const otherMode = entry({ snoozed: false, archived: true }, [null, false, true, null, false]);
    const patched = applyBulkRead(
      [inScope, unreadOnly, otherOrigin, otherMode],
      { snoozed: false, archived: false },
      READ_AT
    );
    expect(patched.map(([key]) => key)).toEqual([inScope.key, unreadOnly.key]);
    for (const [, result] of patched) {
      expect(result.results[0]?.read_at).toBe(READ_AT);
    }
  });

  it("carries the origin scope through", () => {
    const created = entry({ type: "created", snoozed: false, archived: false }, ["created", false, false, null, false]);
    const assigned = entry({ type: "assigned", snoozed: false, archived: false }, [
      "assigned",
      false,
      false,
      null,
      false,
    ]);
    const patched = applyBulkRead([created, assigned], { type: "created", snoozed: false, archived: false }, READ_AT);
    expect(patched.map(([key]) => key)).toEqual([created.key]);
  });

  it("patches the snoozed and archived modes' own pages", () => {
    const snoozed = entry({ snoozed: true, archived: false }, [null, true, false, null, false]);
    const archived = entry({ snoozed: false, archived: true }, [null, false, true, null, false]);
    const patched = applyBulkRead([snoozed, archived], { snoozed: true, archived: false }, READ_AT);
    expect(patched.map(([key]) => key)).toEqual([snoozed.key]);
  });

  it("does nothing when no cached page matches", () => {
    const other = entry({ type: "subscribed", snoozed: false, archived: false }, [
      "subscribed",
      false,
      false,
      null,
      false,
    ]);
    expect(applyBulkRead([other], { snoozed: false, archived: false }, READ_AT)).toEqual([]);
  });
});
