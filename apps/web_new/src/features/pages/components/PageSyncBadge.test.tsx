// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sync badge (parity/specs/pages.md §8.2, PAGE-024, PAGE-061): the two
// pinned words and nothing at all once the document is settled. The last
// word lingers a short beat after a synced report before hiding, and the
// fault note states the real background-save cadence.

import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PAGE_SYNC_LOST, PAGE_SYNC_SYNCING } from "../lib/pageSync.js";
import { PageSyncBadge, PAGE_SYNC_HIDE_MS, PAGE_SYNC_LOST_HINT } from "./PageSyncBadge.js";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("PageSyncBadge", () => {
  it("shows the transitional word while connecting", () => {
    render(<PageSyncBadge status="syncing" />);
    expect(screen.getByText(PAGE_SYNC_SYNCING)).toBeInTheDocument();
  });

  it("shows the stable fault badge with its honest hint", () => {
    render(<PageSyncBadge status="lost" />);
    expect(screen.getByText(PAGE_SYNC_LOST)).toHaveAttribute("title", PAGE_SYNC_LOST_HINT);
    expect(PAGE_SYNC_LOST_HINT).toContain("background");
    expect(PAGE_SYNC_LOST_HINT).toContain("30 seconds");
  });

  it("renders nothing once synced", () => {
    const { container } = render(<PageSyncBadge status="synced" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lingers the last word a beat after a synced report, then hides", () => {
    const { rerender } = render(<PageSyncBadge status="syncing" />);
    rerender(<PageSyncBadge status="synced" />);
    expect(screen.getByText(PAGE_SYNC_SYNCING)).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_HIDE_MS);
    });
    expect(screen.queryByText(PAGE_SYNC_SYNCING)).not.toBeInTheDocument();
  });

  it("lingers a cleared fault the same way", () => {
    const { rerender } = render(<PageSyncBadge status="lost" />);
    rerender(<PageSyncBadge status="synced" />);
    expect(screen.getByText(PAGE_SYNC_LOST)).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_HIDE_MS);
    });
    expect(screen.queryByText(PAGE_SYNC_LOST)).not.toBeInTheDocument();
  });

  it("cancels the hide when syncing resumes mid-beat", () => {
    const { rerender } = render(<PageSyncBadge status="syncing" />);
    rerender(<PageSyncBadge status="synced" />);
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_HIDE_MS - 1);
    });
    rerender(<PageSyncBadge status="syncing" />);
    act(() => {
      vi.advanceTimersByTime(PAGE_SYNC_HIDE_MS);
    });
    expect(screen.getByText(PAGE_SYNC_SYNCING)).toBeInTheDocument();
  });
});
