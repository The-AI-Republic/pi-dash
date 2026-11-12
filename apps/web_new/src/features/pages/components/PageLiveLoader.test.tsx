// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration sync loader (parity/specs/pages.md §8.4, PAGE-067): the
// status region the surface shows while the cache is known-empty and
// the document is unsynced.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PageLiveLoader } from "./PageLiveLoader.js";

describe("PageLiveLoader", () => {
  it("announces the sync wait", () => {
    render(<PageLiveLoader />);
    const loader = screen.getByTestId("page-live-loader");
    expect(loader).toHaveAttribute("role", "status");
    expect(loader).toHaveAccessibleName("Syncing page content");
    expect(screen.getByTestId("page-detail-spinner")).toBeInTheDocument();
  });
});
