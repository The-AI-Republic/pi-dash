// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored-embed heal planner (parity/specs/pages.md §6.2, PAGE-040,
// NEWFRONT-932): the snapshot embeds a mounted document lacks, in
// snapshot order, with the index each one restores.

import { describe, expect, it } from "vitest";

import { missingPageEmbedInserts } from "./pageEmbedHeal.js";

const IDENTIFIERS = {
  id: "emb-oracle-1",
  entity_identifier: "N316-ISS-1",
  project_identifier: "N316-PROJ",
  workspace_identifier: "acme",
  entity_name: "Embedded issue",
};

function paragraph(text: string): Record<string, unknown> {
  return { type: "paragraph", content: [{ type: "text", text }] };
}

function embed(attrs: Record<string, unknown> = IDENTIFIERS): Record<string, unknown> {
  return { type: "issue-embed-component", attrs };
}

function doc(...content: unknown[]): Record<string, unknown> {
  return { type: "doc", content };
}

describe("missingPageEmbedInserts", () => {
  it("heals nothing when the snapshot carries no embeds", () => {
    const document = doc(paragraph("before embed"));
    expect(missingPageEmbedInserts(null, document)).toEqual([]);
    expect(missingPageEmbedInserts("<p>nope</p>", document)).toEqual([]);
    expect(missingPageEmbedInserts({}, document)).toEqual([]);
    expect(missingPageEmbedInserts({ type: "doc" }, document)).toEqual([]);
    expect(missingPageEmbedInserts({ type: "doc", content: "nope" }, document)).toEqual([]);
    expect(missingPageEmbedInserts(doc(paragraph("words")), document)).toEqual([]);
  });

  it("skips entries that are not nodes", () => {
    expect(missingPageEmbedInserts(doc(null, 42, "para", paragraph("words")), doc())).toEqual([]);
  });

  it("names a JSON-only embed with its snapshot index and identifiers", () => {
    const snapshot = doc(paragraph("before embed"), embed(), paragraph("after embed"));
    expect(missingPageEmbedInserts(snapshot, doc(paragraph("before embed")))).toEqual([
      { attributes: IDENTIFIERS, index: 1 },
    ]);
    // A document that is not JSON at all carries nothing to match.
    expect(missingPageEmbedInserts(snapshot, null)).toHaveLength(1);
  });

  it("heals nothing when the document already carries the embed", () => {
    const snapshot = doc(paragraph("before embed"), embed());
    const document = doc(paragraph("before embed"), embed());
    expect(missingPageEmbedInserts(snapshot, document)).toEqual([]);
  });

  it("counts a nested document embed as present", () => {
    const snapshot = doc(embed());
    const document = doc({ type: "blockquote", content: [embed()] });
    expect(missingPageEmbedInserts(snapshot, document)).toEqual([]);
  });

  it("matches by stored id when the work-item identifier is empty", () => {
    const snapshot = doc(embed({ id: "emb-9" }));
    expect(missingPageEmbedInserts(snapshot, doc())).toEqual([
      {
        attributes: {
          id: "emb-9",
          entity_identifier: "",
          project_identifier: "",
          workspace_identifier: "",
          entity_name: "",
        },
        index: 0,
      },
    ]);
    expect(missingPageEmbedInserts(snapshot, doc(embed({ id: "emb-9" })))).toEqual([]);
  });

  it("restores each occurrence of a twice-embedded item the document lacks", () => {
    const snapshot = doc(embed(), paragraph("between"), embed());
    expect(missingPageEmbedInserts(snapshot, doc())).toHaveLength(2);
    const [first, second] = missingPageEmbedInserts(snapshot, doc());
    expect(first?.index).toBe(0);
    expect(second?.index).toBe(2);
    expect(missingPageEmbedInserts(snapshot, doc(embed()))).toHaveLength(1);
  });

  it("narrows missing and non-string attributes to empty strings", () => {
    const snapshot = doc({ type: "issue-embed-component" }, { type: "issue-embed-component", attrs: "nope" });
    expect(missingPageEmbedInserts(snapshot, doc())).toEqual([
      {
        attributes: {
          id: "",
          entity_identifier: "",
          project_identifier: "",
          workspace_identifier: "",
          entity_name: "",
        },
        index: 0,
      },
      {
        attributes: {
          id: "",
          entity_identifier: "",
          project_identifier: "",
          workspace_identifier: "",
          entity_name: "",
        },
        index: 1,
      },
    ]);
    const numeric = doc(embed({ ...IDENTIFIERS, entity_name: 7 }));
    const [insert] = missingPageEmbedInserts(numeric, doc());
    expect(insert?.attributes.entity_name).toBe("");
    expect(insert?.attributes.entity_identifier).toBe("N316-ISS-1");
  });
});
