// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: public board issue reads against live Django (NEWFRONT-752).
// Pins the grouped envelope the board renders, the flat row shape (no
// nested state/votes), per-group cursor paging, filter parameters and the
// detail payload.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { getSpaceBoardIssue, listSpaceBoardIssues } from "../contracts/space-issues.js";
import { listSpaceBoardStates } from "../contracts/space-display.js";
import { CONTRACT_BOARD_ANCHOR, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("space issue contract", () => {
  it("reads the grouped board with flat rows and per-group totals", async () => {
    const client = await sharedSignedIn();
    const page = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
      orderBy: "-created_at",
    });
    expect(page.grouped_by).toBe("state_id");
    expect(typeof page.total_count).toBe("number");
    expect(typeof page.next_cursor).toBe("string");
    expect(page.results).toBeTypeOf("object");
    const buckets = Object.values(page.results);
    expect(buckets.length).toBeGreaterThan(0);
    for (const bucket of buckets) {
      expect(bucket.total_results).toBeTypeOf("number");
    }
    const rows = buckets.flatMap((bucket) => bucket.results);
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row).not.toHaveProperty("state");
      expect(row).not.toHaveProperty("project");
      expect(row).not.toHaveProperty("votes");
      expect(Array.isArray(row.vote_items)).toBe(true);
      expect(Array.isArray(row.reaction_items)).toBe(true);
      expect(Array.isArray(row.label_ids)).toBe(true);
    }
  });

  it("filters the board by state id", async () => {
    const client = await sharedSignedIn();
    const states = await listSpaceBoardStates(client, CONTRACT_BOARD_ANCHOR);
    expect(states.length).toBeGreaterThan(0);
    const target = states[0];
    const page = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
      state: [target?.id ?? ""],
    });
    const buckets = Object.values(page.results);
    expect(buckets.length).toBeGreaterThan(0);
    for (const row of buckets.flatMap((bucket) => bucket.results)) {
      expect(row.state_id).toBe(target?.id);
    }
  });

  it("pages a single group with the group's next cursor", async () => {
    const client = await sharedSignedIn();
    const first = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
      groupBy: "state_id",
      perPage: 1,
      cursor: "1:0:0",
    });
    const rows = Object.values(first.results).flatMap((bucket) => bucket.results);
    expect(rows.length).toBeGreaterThan(0);
    const next = first.next_cursor;
    if (next) {
      const second = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
        groupBy: "state_id",
        perPage: 1,
        cursor: next,
      });
      const nextRows = Object.values(second.results).flatMap((bucket) => bucket.results);
      const firstIds = new Set(rows.map((row) => row.id));
      for (const row of nextRows) expect(firstIds.has(row.id)).toBe(false);
    }
  });

  it("reads one issue's detail with its description fields", async () => {
    const client = await sharedSignedIn();
    const page = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
    });
    const row = Object.values(page.results).flatMap((bucket) => bucket.results)[0];
    expect(row).toBeDefined();
    const detail = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, row?.id ?? "");
    expect(detail.id).toBe(row?.id);
    expect(typeof detail.description_html).toBe("string");
    expect(detail).not.toHaveProperty("votes");
    expect(Array.isArray(detail.vote_items)).toBe(true);
  });

  it("answers 404 for an anchor that is not published", async () => {
    const error = await listSpaceBoardIssues(await sharedSignedIn(), "no-such-anchor-752").catch(
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
  });
});
