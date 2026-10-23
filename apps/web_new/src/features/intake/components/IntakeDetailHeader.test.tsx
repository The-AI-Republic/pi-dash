// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail header (INT-024/027): chevron steps, the copy/open
// split between open and resolved requests, and the slice E slot.

import { ToastHost } from "@pidash/kit";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

const platformHarness = vi.hoisted(() => ({
  shareableUrl: vi.fn((path: string) => `https://app.example.test${path}`),
}));
vi.mock("../../../core/platform/index.js", () => ({
  platform: { shareableUrl: (path: string) => platformHarness.shareableUrl(path) },
}));

import { getToastManager } from "../../../shared/shell/toaster.js";
import {
  NEXT_REQUEST_LABEL,
  OPEN_WORK_ITEM_LABEL,
  PREV_REQUEST_LABEL,
  REQUEST_ACTIONS_LABEL,
  IntakeDetailHeader,
} from "./IntakeDetailHeader.js";

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

function renderHeader(props: Partial<React.ComponentProps<typeof IntakeDetailHeader>> = {}) {
  const onPrev = vi.fn();
  const onNext = vi.fn();
  render(
    <>
      <ToastHost manager={getToastManager()} />
      <IntakeDetailHeader
        workspaceSlug="acme"
        issueKey="APO-7"
        resolved={false}
        canCopy
        onPrev={onPrev}
        onNext={onNext}
        {...props}
      />
    </>
  );
  return { onPrev, onNext };
}

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("IntakeDetailHeader chevrons (INT-027)", () => {
  it("steps to the neighbours", () => {
    const { onPrev, onNext } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PREV_REQUEST_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: NEXT_REQUEST_LABEL }));
    expect(onPrev).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledTimes(1);
  });
});

describe("IntakeDetailHeader link controls (INT-024)", () => {
  it("copies from the overflow menu on an open request", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    renderHeader();
    expect(screen.getByTestId("intake-detail-menu")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: REQUEST_ACTIONS_LABEL }));
    await user.click(await screen.findByRole("menuitem", { name: "Copy work item link" }));
    expect(writeText).toHaveBeenCalledWith("https://app.example.test/acme/browse/APO-7/");
    expect(await screen.findByText("Copied the work item link.")).toBeInTheDocument();
  });

  it("shows no copy control to a viewer without copy standing on an open request", () => {
    renderHeader({ canCopy: false });
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy work item link" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-detail-open-work-item")).not.toBeInTheDocument();
  });

  it("shows direct copy and open controls to every viewer on a resolved request", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    renderHeader({ resolved: true, canCopy: false });
    expect(screen.queryByTestId("intake-detail-menu")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy work item link" }));
    expect(writeText).toHaveBeenCalledWith("https://app.example.test/acme/browse/APO-7/");
    expect(await screen.findByText("Copied the work item link.")).toBeInTheDocument();
    const open = screen.getByTestId("intake-detail-open-work-item");
    expect(open).toHaveTextContent(OPEN_WORK_ITEM_LABEL);
    expect(open).toHaveAttribute("href", "/acme/browse/APO-7/");
  });

  it("renders the triage action slot", () => {
    renderHeader({ actions: <button type="button">Accept</button> });
    expect(screen.getByRole("button", { name: "Accept" })).toBeInTheDocument();
  });
});
