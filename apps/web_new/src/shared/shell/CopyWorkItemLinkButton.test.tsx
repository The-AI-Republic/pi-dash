// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared detail/peek copy control (DESK-019): one click copies the
// canonical work-item address through the platform, and a refused copy
// surfaces the failure toast instead of a stale or private link.

import { ToastHost } from "@pidash/kit";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const platformHarness = vi.hoisted(() => ({
  shareableUrl: vi.fn((path: string) => `https://app.example.test${path}`),
}));
vi.mock("../../core/platform/index.js", () => ({
  platform: { shareableUrl: (path: string) => platformHarness.shareableUrl(path) },
}));

import { COPY_WORK_ITEM_LINK_LABEL, CopyWorkItemLinkButton } from "./CopyWorkItemLinkButton.js";
import { getToastManager } from "./toaster.js";

beforeEach(() => {
  platformHarness.shareableUrl.mockImplementation((path: string) => `https://app.example.test${path}`);
});

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

function renderButton(): void {
  render(
    <>
      <ToastHost manager={getToastManager()} />
      <CopyWorkItemLinkButton workspaceSlug="acme" issueKey="ACME-12" />
    </>
  );
}

describe("CopyWorkItemLinkButton", () => {
  it("copies the hosted canonical address and confirms", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    renderButton();
    // fireEvent (not user-event, which installs its own clipboard stub) so
    // the component writes through the spy the test owns.
    fireEvent.click(screen.getByRole("button", { name: COPY_WORK_ITEM_LINK_LABEL }));
    expect(platformHarness.shareableUrl).toHaveBeenCalledWith("/acme/browse/ACME-12/");
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://app.example.test/acme/browse/ACME-12/"));
    expect(await screen.findByText("Copied the work item link.")).toBeInTheDocument();
  });

  it("toasts the refusal when no hosted origin exists", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    platformHarness.shareableUrl.mockImplementation(() => {
      throw new Error("A hosted web origin is required to create a shareable link.");
    });
    renderButton();
    fireEvent.click(screen.getByRole("button", { name: COPY_WORK_ITEM_LINK_LABEL }));
    expect(await screen.findByText("Couldn't copy the work item link.")).toBeInTheDocument();
    expect(screen.getByText("A hosted web origin is required to create a shareable link.")).toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
  });
});
