// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectQuickActions } from "./ProjectQuickActions.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");

function stubClipboard(writeText: () => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
}

function setup(
  standing: { isAdmin: boolean; isAuthorized: boolean } = { isAdmin: true, isAuthorized: true },
  activeHref: string | null = `/acme/projects/${PROJECT_ID}/pages`
) {
  const onNavigate = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectQuickActions
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Web"
        activeHref={activeHref}
        isAdmin={standing.isAdmin}
        isAuthorized={standing.isAuthorized}
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { onNavigate };
}

function openMenu(): void {
  fireEvent.click(screen.getByTestId("project-actions-trigger"));
}

function menuNames(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
});

// Both dialogs ride lazily imported chunks; the first import can take
// seconds while the whole suite competes for the CPU. The test timeout
// sits above the find budget so vitest does not stop the test first.
const LAZY_DIALOG_TIMEOUT = { timeout: 10_000 };
const LAZY_DIALOG_TEST_TIMEOUT = 30_000;

describe("ProjectQuickActions", { timeout: LAZY_DIALOG_TEST_TIMEOUT }, () => {
  it("offers admins publish, copy, archives and settings", () => {
    setup({ isAdmin: true, isAuthorized: true });
    openMenu();
    expect(menuNames()).toEqual(["Publish project", "Copy link", "Archives", "Settings"]);
  });

  it("offers members copy, archives and settings", () => {
    setup({ isAdmin: false, isAuthorized: true });
    openMenu();
    expect(menuNames()).toEqual(["Copy link", "Archives", "Settings"]);
  });

  it("offers outsiders copy, settings and leave", () => {
    setup({ isAdmin: false, isAuthorized: false });
    openMenu();
    expect(menuNames()).toEqual(["Copy link", "Settings", "Leave project"]);
  });

  it("opens the publish dialog from its entry", async () => {
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Publish project" }));
    await vi.dynamicImportSettled();
    expect(await screen.findByRole("heading", { name: "Publish project" }, LAZY_DIALOG_TIMEOUT)).toBeInTheDocument();
  });

  it("opens the leave dialog from its entry", async () => {
    setup({ isAdmin: false, isAuthorized: false });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Leave project" }));
    await vi.dynamicImportSettled();
    expect(await screen.findByRole("heading", { name: "Leave project" }, LAZY_DIALOG_TIMEOUT)).toBeInTheDocument();
  });

  it("navigates to archives and settings from their entries", () => {
    const { onNavigate } = setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archives" }));
    expect(onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/archives/issues`);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(onNavigate).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_ID}`);
  });

  it("copies the active tab address with a notice", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(`http://localhost:3000/acme/projects/${PROJECT_ID}/pages`)
    );
    expect(notifySuccess).toHaveBeenCalledWith("Copied the project link", expect.any(String));
  });

  it("falls back to the tracking address without an active tab", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    setup({ isAdmin: true, isAuthorized: true }, null);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(`http://localhost:3000/acme/projects/${PROJECT_ID}/issues`)
    );
  });

  it("toasts distinctly when the clipboard write is refused", async () => {
    stubClipboard(vi.fn().mockRejectedValue(new Error("denied")));
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the address", expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts distinctly without a clipboard API", async () => {
    Object.defineProperty(globalThis.navigator, "clipboard", { value: undefined, configurable: true });
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the address", expect.any(String)));
  });
});
