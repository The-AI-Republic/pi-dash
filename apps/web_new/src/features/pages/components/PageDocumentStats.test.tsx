// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Statistics cards (parity/specs/pages.md §6.8, PAGE-049): words,
// characters, paragraphs and the reading-time estimate.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PageDocumentStats } from "./PageDocumentStats.js";

function statValue(label: string): string | null {
  const card = screen.getByText(label).parentElement;
  return card?.querySelector("h6")?.textContent ?? null;
}

describe("PageDocumentStats", () => {
  it("shows the counts with a seconds read time under a minute", () => {
    render(<PageDocumentStats stats={{ words: 100, characters: 640, paragraphs: 3 }} />);
    expect(statValue("Words")).toBe("100");
    expect(statValue("Characters")).toBe("640");
    expect(statValue("Paragraphs")).toBe("3");
    expect(statValue("Read time")).toBe("30s");
  });

  it("shows the read time in minutes for a long document", () => {
    render(<PageDocumentStats stats={{ words: 1201, characters: 7000, paragraphs: 40 }} />);
    expect(statValue("Read time")).toBe("6m");
  });
});
