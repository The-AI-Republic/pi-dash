// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mention helpers (parity/specs/issues.md §11, ISS-127). The stored
// markup must match what every client renders, so the Tiptap node's
// parse/render pair is pinned in mentionNode.test.ts against the real
// loaded node, and the pure helpers live here.

import { describe, expect, it } from "vitest";

import { MENTION_CONTENT_TAGS, MENTION_ENTITY, MENTION_TAG, mentionAttributes, mentionQueryAt } from "./mention.js";

describe("mentionAttributes", () => {
  it("keeps the entity kind stable for other clients", () => {
    expect(MENTION_ENTITY).toBe("user_mention");
    expect(mentionAttributes({ id: "u-1", name: "Ada" })).toEqual({
      id: "u-1",
      entity_identifier: "u-1",
      entity_name: "user_mention",
    });
  });
});

describe("mentionQueryAt", () => {
  it("reads the text typed after a leading @", () => {
    expect(mentionQueryAt("@ad")).toEqual({ text: "ad" });
  });

  it("opens on an empty query right after the @", () => {
    expect(mentionQueryAt("hello @")).toEqual({ text: "" });
  });

  it("ignores an @ glued to a word", () => {
    expect(mentionQueryAt("mail@example")).toBeNull();
  });

  it("ignores a query broken by whitespace", () => {
    expect(mentionQueryAt("@ada grace")).toBeNull();
  });

  it("returns null without any @", () => {
    expect(mentionQueryAt("plain words")).toBeNull();
  });

  it("uses the nearest @ on the run", () => {
    expect(mentionQueryAt("ping @ada and @gr")).toEqual({ text: "gr" });
  });
});

describe("MENTION_CONTENT_TAGS", () => {
  it("counts images and mentions as description content", () => {
    expect(MENTION_CONTENT_TAGS).toContain("img");
    expect(MENTION_CONTENT_TAGS).toContain(MENTION_TAG);
  });
});
