// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Version overlay (parity/specs/pages.md §4.5, §6.9, PAGE-052/054/079):
// the read-only marker, the loading title, the snapshot skeleton while its
// read is in flight, the frozen structured-then-markup body, the failure
// panel with its retry, the close control, and the restore action that
// exists only when the shell hands one down (PAGE-053).

import { fireEvent, render, screen } from "@testing-library/react";
import type { PageVersionDetail } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  PageVersionOverlay,
  PAGE_VERSION_CLOSE,
  PAGE_VERSION_ERROR,
  PAGE_VERSION_ERROR_HINT,
  PAGE_VERSION_LOADING,
  PAGE_VERSION_RESTORE,
  PAGE_VERSION_RETRY,
  PAGE_VERSION_VIEW_ONLY,
} from "./PageVersionOverlay.js";

function structuredDoc(text: string): Record<string, unknown> {
  return { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text }] }] };
}

const SNAPSHOT: PageVersionDetail = {
  id: "11111111-1111-4111-8111-111111111111",
  workspace: "22222222-2222-4222-8222-222222222222",
  page: "33333333-3333-4333-8333-333333333333",
  last_saved_at: "2026-10-20T12:00:00Z",
  description_binary: null,
  description_html: "<p>Stored snapshot body</p>",
  description_json: structuredDoc("Structured snapshot body"),
  owned_by: "44444444-4444-4444-8444-444444444444",
  created_at: "2026-10-20T12:00:00Z",
  updated_at: "2026-10-20T12:00:00Z",
  created_by: null,
  updated_by: null,
};

function overlay(
  snapshot: PageVersionDetail | undefined,
  options: { failed?: boolean; onRestore?: (() => void) | null } = {}
): {
  onClose: ReturnType<typeof vi.fn>;
  onRetry: ReturnType<typeof vi.fn>;
  container: HTMLElement;
} {
  const onClose = vi.fn();
  const onRetry = vi.fn();
  const restore =
    options.onRestore === null ? null : options.onRestore === undefined ? undefined : { onRestore: options.onRestore };
  const { container } = render(
    <PageVersionOverlay
      versionId="version-1"
      snapshot={snapshot}
      failed={options.failed ?? false}
      onClose={onClose}
      onRetry={onRetry}
      restore={restore}
    />
  );
  return { onClose, onRetry, container };
}

describe("PageVersionOverlay", () => {
  it("marks itself read-only and titles the snapshot with an h6", () => {
    overlay(SNAPSHOT);
    expect(screen.getByText(PAGE_VERSION_VIEW_ONLY)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent("Oct 20, 2026");
  });

  it("covers the canvas rather than the window, so the page header stays reachable", () => {
    // §4.5: "a full-canvas overlay above the editor". The header's own
    // controls (copy link, the switcher) sit outside it and stay clickable
    // while a snapshot is open, so the overlay fills its containing column
    // and never claims the whole window or traps the page as a modal.
    overlay(SNAPSHOT);
    const surface = screen.getByRole("dialog", { name: "Version snapshot" });
    expect(surface).toHaveClass("absolute", "inset-0");
    expect(surface.className).not.toContain("fixed");
    expect(surface).not.toHaveAttribute("aria-modal");
  });

  it("renders the frozen structured snapshot without an editable region", () => {
    const { container } = overlay(SNAPSHOT);
    expect(screen.getByText("Structured snapshot body")).toBeInTheDocument();
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
    expect(container.querySelector("[data-version-id='version-1']")).not.toBeNull();
  });

  it("falls back to the stored markup when the structured form is empty", () => {
    overlay({ ...SNAPSHOT, description_json: {} });
    expect(screen.getByText("Stored snapshot body")).toBeInTheDocument();
  });

  it("skeletons the body and names the pending read until the snapshot settles", () => {
    overlay(undefined);
    expect(screen.getByRole("status", { name: "Loading version" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent(PAGE_VERSION_LOADING);
  });

  it("panels a failed snapshot read with the pinned copy and a retry", () => {
    const { onRetry } = overlay(undefined, { failed: true });
    expect(screen.getByRole("heading", { name: PAGE_VERSION_ERROR })).toBeInTheDocument();
    expect(screen.getByText(PAGE_VERSION_ERROR_HINT)).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: PAGE_VERSION_RETRY }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("closes from the header control", () => {
    const { onClose } = overlay(SNAPSHOT);
    fireEvent.click(screen.getByRole("button", { name: PAGE_VERSION_CLOSE }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("offers no restore action when the shell hands none down", () => {
    overlay(SNAPSHOT);
    expect(screen.queryByRole("button", { name: PAGE_VERSION_RESTORE })).not.toBeInTheDocument();
  });

  it("runs the handed-down restore from the header", () => {
    const onRestore = vi.fn();
    overlay(SNAPSHOT, { onRestore });
    fireEvent.click(screen.getByRole("button", { name: PAGE_VERSION_RESTORE }));
    expect(onRestore).toHaveBeenCalledTimes(1);
  });

  it("withholds the restore until the snapshot it would write is known", () => {
    const onRestore = vi.fn();
    overlay(undefined, { onRestore });
    expect(screen.queryByRole("button", { name: PAGE_VERSION_RESTORE })).not.toBeInTheDocument();
  });

  it("withholds the restore from the failed-read panel", () => {
    const onRestore = vi.fn();
    overlay(undefined, { failed: true, onRestore });
    expect(screen.queryByRole("button", { name: PAGE_VERSION_RESTORE })).not.toBeInTheDocument();
  });

  it("shows an unreadable timestamp verbatim", () => {
    overlay({ ...SNAPSHOT, last_saved_at: "not a date" });
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent("not a date");
  });
});
