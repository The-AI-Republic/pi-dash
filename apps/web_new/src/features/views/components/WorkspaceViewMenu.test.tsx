// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view quick menu (parity/specs/views.md §10, VIEW-037).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", () => notify);

const platformHarness = vi.hoisted(() => ({
  shareableUrl: vi.fn((path: string) => `https://app.example.test${path}`),
  openExternal: vi.fn(async (url: string): Promise<void> => {
    void url;
  }),
}));
vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    shareableUrl: (path: string) => platformHarness.shareableUrl(path),
    openExternal: (url: string) => platformHarness.openExternal(url),
  },
}));

import { LIST_COPY_MESSAGE } from "../../issues/index.js";
import {
  WorkspaceViewMenu,
  selectWorkspaceViewMenuEntries,
  type WorkspaceViewMenuStanding,
} from "./WorkspaceViewMenu.js";

const HREF = "/acme/workspace-views/323e4567-e89b-12d3-a456-426614174002";

function callbacks(overrides: { onEdit?: (() => void) | null; onDelete?: (() => void) | null } = {}): {
  onEdit: (() => void) | null;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
  onDelete: (() => void) | null;
} {
  return {
    onEdit: overrides.onEdit === undefined ? vi.fn() : overrides.onEdit,
    onOpenInNewTab: vi.fn(),
    onCopyLink: vi.fn(),
    onDelete: overrides.onDelete === undefined ? vi.fn() : overrides.onDelete,
  };
}

function labels(standing: WorkspaceViewMenuStanding, withCallbacks = callbacks()): string[] {
  return selectWorkspaceViewMenuEntries(standing, withCallbacks)
    .map((entry) => ("label" in entry ? entry.label : null))
    .filter((label): label is string => label !== null);
}

afterEach(() => {
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
  platformHarness.shareableUrl.mockImplementation((path: string) => `https://app.example.test${path}`);
  platformHarness.openExternal.mockClear();
  vi.unstubAllGlobals();
});

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

async function openMenu(): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha" }));
  await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
}

describe("selectWorkspaceViewMenuEntries", () => {
  it("offers the full set to the owner in order", () => {
    expect(labels({ isOwner: true, canDelete: true })).toEqual(["Edit", "Open in new tab", "Copy link", "Delete"]);
  });

  it("offers open and copy only to a non-owner member", () => {
    expect(labels({ isOwner: false, canDelete: false })).toEqual(["Open in new tab", "Copy link"]);
  });

  it("adds delete for a non-owner admin", () => {
    expect(labels({ isOwner: false, canDelete: true })).toEqual(["Open in new tab", "Copy link", "Delete"]);
  });

  it("hides edit and delete until their dialogs wire up", () => {
    expect(labels({ isOwner: true, canDelete: true }, callbacks({ onEdit: null, onDelete: null }))).toEqual([
      "Open in new tab",
      "Copy link",
    ]);
  });
});

describe("WorkspaceViewMenu", () => {
  it("names its trigger after the view it acts on", () => {
    render(
      <WorkspaceViewMenu
        href={HREF}
        standing={{ isOwner: true, canDelete: true }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        triggerLabel="Actions for Alpha"
      />
    );
    expect(screen.getByRole("button", { name: "Actions for Alpha" })).toBeInTheDocument();
  });

  it("runs the edit and delete handlers", async () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    render(
      <WorkspaceViewMenu
        href={HREF}
        standing={{ isOwner: true, canDelete: true }}
        onEdit={onEdit}
        onDelete={onDelete}
        triggerLabel="Actions for Alpha"
      />
    );
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledTimes(1);
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it("opens the absolute detail address in a new tab", async () => {
    render(
      <WorkspaceViewMenu
        href={HREF}
        standing={{ isOwner: false, canDelete: false }}
        onEdit={null}
        onDelete={null}
        triggerLabel="Actions for Alpha"
      />
    );
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(platformHarness.shareableUrl).toHaveBeenCalledWith(HREF);
    expect(platformHarness.openExternal).toHaveBeenCalledWith(`https://app.example.test${HREF}`);
  });

  it("copies the detail address and confirms with the shared list wording", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    render(
      <WorkspaceViewMenu
        href={HREF}
        standing={{ isOwner: false, canDelete: false }}
        onEdit={null}
        onDelete={null}
        triggerLabel="Actions for Alpha"
      />
    );
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`https://app.example.test${HREF}`));
    expect(notify.notifySuccess).toHaveBeenCalledWith("Link Copied", LIST_COPY_MESSAGE);
    // ISS-067 pins this fragment on the toast body.
    expect(LIST_COPY_MESSAGE).toContain("link copied to clipboard");
  });

  it("toasts the refusal when the address cannot be produced", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    platformHarness.shareableUrl.mockImplementation(() => {
      throw new Error("A hosted web origin is required to create a shareable link.");
    });
    render(
      <WorkspaceViewMenu
        href={HREF}
        standing={{ isOwner: false, canDelete: false }}
        onEdit={null}
        onDelete={null}
        triggerLabel="Actions for Alpha"
      />
    );
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Couldn't copy the view link.",
        "A hosted web origin is required to create a shareable link."
      )
    );
    expect(writeText).not.toHaveBeenCalled();
  });
});
