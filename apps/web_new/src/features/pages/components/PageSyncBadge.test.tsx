// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sync badge (parity/specs/pages.md §8.2, PAGE-024): the two pinned words
// and nothing at all once the document is settled.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PAGE_SYNC_LOST, PAGE_SYNC_SYNCING } from "../lib/pageSync.js";
import { PageSyncBadge, PAGE_SYNC_LOST_HINT } from "./PageSyncBadge.js";

describe("PageSyncBadge", () => {
  it("shows the transitional word while connecting", () => {
    render(<PageSyncBadge status="syncing" />);
    expect(screen.getByText(PAGE_SYNC_SYNCING)).toBeInTheDocument();
  });

  it("shows the stable fault badge with its honest hint", () => {
    render(<PageSyncBadge status="lost" />);
    expect(screen.getByText(PAGE_SYNC_LOST)).toHaveAttribute("title", PAGE_SYNC_LOST_HINT);
    expect(PAGE_SYNC_LOST_HINT).toContain("background");
  });

  it("renders nothing once synced", () => {
    const { container } = render(<PageSyncBadge status="synced" />);
    expect(container).toBeEmptyDOMElement();
  });
});
