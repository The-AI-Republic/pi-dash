// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { ROW_LINK_COPIED_TITLE, SidebarProjectQuickMenu } from "./SidebarProjectQuickMenu.js";

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: PROJECT_ID,
    name: "Atlas",
    identifier: "ATL",
    sort_order: 10000,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");
const assign = vi.fn();

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
}

function setup(props: { role?: number | null; onActionNavigate?: () => void } = {}): {
  onNavigate: ReturnType<typeof vi.fn>;
  onActionNavigate: ReturnType<typeof vi.fn>;
} {
  const onNavigate = vi.fn();
  const onActionNavigate = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SidebarProjectQuickMenu
        workspaceSlug="acme"
        project={project({ member_role: props.role === undefined ? 20 : props.role })}
        onNavigate={onNavigate}
        onActionNavigate={props.onActionNavigate ?? onActionNavigate}
      />
    </QueryClientProvider>
  );
  return { onNavigate, onActionNavigate };
}

function menuNames(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

function openMenu(): void {
  fireEvent.click(screen.getByRole("button", { name: "Actions for Atlas" }));
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", (() => {
    throw new Error("quick menu must not fetch");
  }) as never);
  Object.defineProperty(globalThis.window, "location", {
    value: { assign, origin: "https://app.test" },
    writable: true,
    configurable: true,
  });
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
});

describe("SidebarProjectQuickMenu entries", () => {
  it("offers admins publish, copy, archives and settings", () => {
    setup({ role: 20 });
    openMenu();
    expect(menuNames()).toEqual(["Publish project", "Copy link", "Archives", "Settings"]);
  });

  it("offers members copy, archives and settings", () => {
    setup({ role: 15 });
    openMenu();
    expect(menuNames()).toEqual(["Copy link", "Archives", "Settings"]);
  });

  it("offers project guests copy, settings and leave", () => {
    setup({ role: 5 });
    openMenu();
    expect(menuNames()).toEqual(["Copy link", "Settings", "Leave project"]);
  });

  it("offers outsiders copy, settings and leave", () => {
    setup({ role: null });
    openMenu();
    expect(menuNames()).toEqual(["Copy link", "Settings", "Leave project"]);
  });
});

describe("SidebarProjectQuickMenu actions", () => {
  it("opens the publish dialog from its entry", async () => {
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Publish project" }));
    expect(await screen.findByRole("heading", { name: "Publish project" })).toBeInTheDocument();
  });

  it("opens the leave dialog from its entry", async () => {
    setup({ role: 5 });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Leave project" }));
    expect(await screen.findByRole("heading", { name: "Leave project" })).toBeInTheDocument();
  });

  it("copies the tracking address with confirmation", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`https://app.test/acme/projects/${PROJECT_ID}/issues`));
    const { notifySuccess } = await import("../../../shared/shell/toaster.js");
    expect(notifySuccess).toHaveBeenCalledWith(ROW_LINK_COPIED_TITLE, expect.any(String));
  });

  it("toasts when the clipboard refuses", async () => {
    stubClipboard(async () => {
      throw new Error("denied");
    });
    setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
  });

  it("loads archives and settings outside the client router, then notifies", () => {
    const h = setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archives" }));
    expect(assign).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/archives/issues`);
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(h.onActionNavigate).toHaveBeenCalledTimes(1);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(assign).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_ID}`);
    expect(h.onActionNavigate).toHaveBeenCalledTimes(2);
  });

  it("navigates without a listener when none is given", () => {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SidebarProjectQuickMenu workspaceSlug="acme" project={project()} onNavigate={() => undefined} />
      </QueryClientProvider>
    );
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archives" }));
    expect(assign).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/archives/issues`);
  });
});
