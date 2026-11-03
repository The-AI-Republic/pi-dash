// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived zero state: the pinned heading plus a supporting line
// that keeps naming archiving, siblings under one card so the parity
// driver reads the card's text minus the heading.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ARCHIVED_CYCLES_EMPTY_HEADING, ArchivedCyclesEmpty } from "./ArchivedCyclesEmpty.js";

describe("ArchivedCyclesEmpty", () => {
  it("pairs the pinned heading with an archiving line under one parent", () => {
    render(<ArchivedCyclesEmpty />);

    const heading = screen.getByRole("heading", { name: ARCHIVED_CYCLES_EMPTY_HEADING });
    expect(heading).toHaveTextContent("No archived cycles yet");
    const card = heading.parentElement;
    expect(card).toHaveAttribute("data-testid", "archived-cycles-empty");
    const detail = (card?.textContent ?? "").replace(heading.textContent ?? "", "").trim();
    expect(detail).toMatch(/archiv/i);
  });

  it("illustrates without adding card text", () => {
    render(<ArchivedCyclesEmpty />);

    const art = screen.getByTestId("archived-cycles-empty").querySelector("img");
    expect(art?.getAttribute("alt")).toBe("");
    expect(art?.getAttribute("aria-hidden")).toBe("true");
  });
});
