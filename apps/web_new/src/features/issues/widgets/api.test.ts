// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { IssueLink, IssueSearchHit } from "@pidash/api-client";
import { candidateKey, linkFailureDetail, newestLinks, normalizeLinkUrl, withoutRelationRow } from "./api.js";
import { emptyRelations, linkRow, relatedRow } from "./widgetTestUtils.js";

describe("widget write helpers", () => {
  it("removes one row from its group and leaves the others", () => {
    const data = { ...emptyRelations(), blocked_by: [relatedRow(), relatedRow({ id: "other" })] };
    const next = withoutRelationRow(data, "blocked_by", relatedRow().id);
    expect(next?.blocked_by).toHaveLength(1);
    expect(next?.blocked_by[0]?.id).toBe("other");
    expect(data.blocked_by).toHaveLength(2);
  });

  it("keeps the read untouched when it has not resolved", () => {
    expect(withoutRelationRow(undefined, "blocking", "x")).toBeUndefined();
  });

  it("prefixes only a scheme-less URL", () => {
    expect(normalizeLinkUrl(" example.com/a ")).toBe("http://example.com/a");
    expect(normalizeLinkUrl("HTTPS://Example.com")).toBe("HTTPS://Example.com");
    expect(normalizeLinkUrl("http://example.com")).toBe("http://example.com");
  });

  it("sorts links newest first without mutating the read", () => {
    const older = linkRow({ id: "a", created_at: "2024-01-01T00:00:00Z" });
    const newer = linkRow({ id: "b", created_at: "2024-01-02T00:00:00Z" });
    const source = [older, newer];
    expect(newestLinks(source).map((link) => link.id)).toEqual(["b", "a"]);
    expect(source[0]).toBe(older);
    expect(newestLinks(undefined)).toEqual([]);
  });

  it("keeps equal timestamps in the read's order", () => {
    const first = linkRow({ id: "a", created_at: "2024-01-01T00:00:00Z" });
    const second = linkRow({ id: "b", created_at: "2024-01-01T00:00:00Z" });
    expect(newestLinks([first, second]).map((link) => link.id)).toEqual(["a", "b"]);
  });

  it("surfaces the error message as the failure detail", () => {
    expect(linkFailureDetail(new Error("duplicate URL"))).toBe("duplicate URL");
  });

  it("builds a picker key from the project identifier and sequence", () => {
    const hit = { project__identifier: "WEB", sequence_id: 9 } as IssueSearchHit;
    expect(candidateKey(hit)).toBe("WEB-9");
  });

  it("exports the link shape the row reads", () => {
    const link: IssueLink = linkRow();
    expect(link.title).toBe("Design doc");
  });
});
