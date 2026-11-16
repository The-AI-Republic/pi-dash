// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  buildSpaceBoardHead,
  SPACE_HEAD_DESCRIPTION_FALLBACK,
  SPACE_HEAD_ROBOTS,
  SPACE_HEAD_TITLE_FALLBACK,
} from "./boardHead.js";

describe("buildSpaceBoardHead", () => {
  it("unfurls the board name and summary with no cover image", () => {
    const claim = buildSpaceBoardHead({
      name: "Public board",
      description: "Team roadmap",
      cover_image: "https://cdn.test/cover.png",
    });
    expect(claim.title).toBe("Public board");
    expect(claim.robots).toBe(SPACE_HEAD_ROBOTS);
    expect(claim.tags).toEqual([
      { tag: "meta", attributes: { name: "description", content: "Team roadmap" } },
      { tag: "meta", attributes: { property: "og:title", content: "Public board" } },
      { tag: "meta", attributes: { property: "og:description", content: "Team roadmap" } },
      { tag: "meta", attributes: { property: "og:type", content: "website" } },
      { tag: "meta", attributes: { name: "twitter:card", content: "summary" } },
      { tag: "meta", attributes: { name: "twitter:title", content: "Public board" } },
      { tag: "meta", attributes: { name: "twitter:description", content: "Team roadmap" } },
    ]);
    for (const tag of claim.tags) {
      expect(JSON.stringify(tag.attributes)).not.toContain("cover.png");
      expect(JSON.stringify(tag.attributes)).not.toContain("image");
    }
  });

  it("falls back per field when the payload is missing, blank or partial", () => {
    for (const meta of [null, undefined, {}, { cover_image: "https://cdn.test/cover.png" }]) {
      const claim = buildSpaceBoardHead(meta);
      expect(claim.title).toBe(SPACE_HEAD_TITLE_FALLBACK);
      expect(claim.tags[0]?.attributes["content"]).toBe(SPACE_HEAD_DESCRIPTION_FALLBACK);
    }
    expect(buildSpaceBoardHead({ name: "   ", description: "" }).title).toBe(SPACE_HEAD_TITLE_FALLBACK);
    const partial = buildSpaceBoardHead({ name: "Named", description: "  " });
    expect(partial.title).toBe("Named");
    expect(partial.tags[0]?.attributes["content"]).toBe(SPACE_HEAD_DESCRIPTION_FALLBACK);
  });
});
