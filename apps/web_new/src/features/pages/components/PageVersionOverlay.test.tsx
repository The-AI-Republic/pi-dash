// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Version overlay (parity/specs/pages.md §4.5, §6.9, PAGE-052/079): the
// read-only marker, the snapshot skeleton while its read is in flight,
// the frozen body, the failure panel and the close control.

import { fireEvent, render, screen } from "@testing-library/react";
import type { PageVersionDetail } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  PageVersionOverlay,
  PAGE_VERSION_CLOSE,
  PAGE_VERSION_ERROR,
  PAGE_VERSION_ERROR_HINT,
  PAGE_VERSION_VIEW_ONLY,
} from "./PageVersionOverlay.js";

const SNAPSHOT: PageVersionDetail = {
  id: "11111111-1111-4111-8111-111111111111",
  workspace: "22222222-2222-4222-8222-222222222222",
  page: "33333333-3333-4333-8333-333333333333",
  last_saved_at: "2026-10-20T12:00:00Z",
  description_binary: null,
  description_html: "<p>Stored snapshot body</p>",
  description_json: {},
  owned_by: "44444444-4444-4444-8444-444444444444",
  created_at: "2026-10-20T12:00:00Z",
  updated_at: "2026-10-20T12:00:00Z",
  created_by: null,
  updated_by: null,
};

function overlay(
  snapshot: PageVersionDetail | undefined,
  failed = false
): { onClose: ReturnType<typeof vi.fn>; container: HTMLElement } {
  const onClose = vi.fn();
  const { container } = render(
    <PageVersionOverlay versionId="version-1" snapshot={snapshot} failed={failed} onClose={onClose} />
  );
  return { onClose, container };
}

describe("PageVersionOverlay", () => {
  it("marks itself read-only and titles the snapshot with an h6", () => {
    overlay(SNAPSHOT);
    expect(screen.getByText(PAGE_VERSION_VIEW_ONLY)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent("Oct 20, 2026");
  });

  it("renders the frozen snapshot markup", () => {
    const { container } = overlay(SNAPSHOT);
    expect(screen.getByText("Stored snapshot body")).toBeInTheDocument();
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
    expect(container.querySelector("[data-version-id='version-1']")).not.toBeNull();
  });

  it("skeletons the body until the snapshot read settles", () => {
    overlay(undefined);
    expect(screen.getByRole("status", { name: "Loading version" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent("Version snapshot");
  });

  it("panels a failed snapshot read", () => {
    overlay(undefined, true);
    expect(screen.getByRole("heading", { name: PAGE_VERSION_ERROR })).toBeInTheDocument();
    expect(screen.getByText(PAGE_VERSION_ERROR_HINT)).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("closes from the header control", () => {
    const { onClose } = overlay(SNAPSHOT);
    fireEvent.click(screen.getByRole("button", { name: PAGE_VERSION_CLOSE }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("falls back to a neutral title for an unreadable timestamp", () => {
    overlay({ ...SNAPSHOT, last_saved_at: "not a date" });
    expect(screen.getByRole("heading", { level: 6 })).toHaveTextContent("Version snapshot");
  });
});
