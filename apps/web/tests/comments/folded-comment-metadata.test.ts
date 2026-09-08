/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const foldLabel = "Folded comment · Click to";

function readSource(relativePath: string): string {
  return readFileSync(new URL(relativePath, import.meta.url), "utf8");
}

function occurrences(source: string, value: string): number {
  return source.split(value).length - 1;
}

describe("folded comment metadata layout", () => {
  it("keeps the Web fold toggle inside the author and timestamp metadata row", () => {
    const source = readSource("../../core/components/comments/card/display.tsx");
    const metadataStart = source.indexOf('<div className="flex flex-1 flex-wrap items-center gap-1">');
    const metadataEnd = source.indexOf("{!disabled &&", metadataStart);
    const metadata = source.slice(metadataStart, metadataEnd);

    expect(metadataStart).toBeGreaterThan(-1);
    expect(metadataEnd).toBeGreaterThan(metadataStart);
    expect(metadata).toContain("calculateTimeAgo(comment.created_at)");
    expect(metadata).toContain(foldLabel);
    expect(metadata).toContain("aria-expanded={isFoldExpanded}");
    expect(metadata).toContain("setIsFoldExpanded");
    expect(occurrences(source, foldLabel)).toBe(1);
  });

  it("keeps the Space fold toggle inside the timestamp metadata row", () => {
    const source = readSource("../../../space/components/issues/peek-overview/comment/comment-detail-card.tsx");
    const metadataStart = source.indexOf('<p className="mt-0.5 flex flex-wrap items-center gap-1');
    const metadataEnd = source.indexOf("</p>", metadataStart);
    const metadata = source.slice(metadataStart, metadataEnd);

    expect(metadataStart).toBeGreaterThan(-1);
    expect(metadataEnd).toBeGreaterThan(metadataStart);
    expect(metadata).toContain("timeAgo(comment.created_at)");
    expect(metadata).toContain(foldLabel);
    expect(metadata).toContain("aria-expanded={isFoldExpanded}");
    expect(metadata).toContain("setIsFoldExpanded");
    expect(occurrences(source, foldLabel)).toBe(1);
  });
});
