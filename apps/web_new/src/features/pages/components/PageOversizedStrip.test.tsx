// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oversized-content strip (parity/specs/pages.md §8.6, PAGE-063): it names
// the paused sync, advises splitting across new or nested pages, and
// dismisses on request.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  PageOversizedStrip,
  PAGE_OVERSIZED_BODY,
  PAGE_OVERSIZED_DISMISS,
  PAGE_OVERSIZED_TITLE,
} from "./PageOversizedStrip.js";

describe("PageOversizedStrip", () => {
  it("advises splitting the content and dismisses on request", () => {
    const onDismiss = vi.fn();
    render(<PageOversizedStrip onDismiss={onDismiss} />);
    const strip = screen.getByTestId("page-oversized-strip");
    expect(strip).toHaveTextContent(PAGE_OVERSIZED_TITLE);
    expect(strip).toHaveTextContent(PAGE_OVERSIZED_BODY);
    expect(PAGE_OVERSIZED_BODY).toContain("nested pages");
    fireEvent.click(screen.getByRole("button", { name: PAGE_OVERSIZED_DISMISS }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
