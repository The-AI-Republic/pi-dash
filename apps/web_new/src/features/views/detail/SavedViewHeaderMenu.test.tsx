// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header overflow menu (parity/specs/views.md §7, VIEW-017 detail
// half). The menu replaces the generic list-page ellipsis on the project
// view detail header: edit for the owner, delete for owner or admin, open
// and copy for everyone, publish on cloud after copy. Copy and open act
// on the canonical view address, never pathname+search.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SavedView } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

const router = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => router.navigate,
}));

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

const platformMocks = vi.hoisted(() => ({
  openExternal: vi.fn(async (_url: string): Promise<void> => undefined),
  shareableUrl: vi.fn((path: string): string => `https://app.test${path}`),
}));
vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    platform: {
      ...((original["platform"] as Record<string, unknown> | undefined) ?? {}),
      openExternal: platformMocks.openExternal,
      shareableUrl: platformMocks.shareableUrl,
    },
  };
});

const share = vi.hoisted(() => ({ copyWorkItemLink: vi.fn(async (): Promise<boolean> => true) }));
vi.mock("../../../shared/shell/shareLink.js", () => share);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { loadRichFilterBuilder } from "../../issues/index.js";
import { makeView } from "../testing.js";
import { viewKeys } from "../api/keys.js";
import {
  SavedViewHeaderMenu,
  VIEW_DELETED_TITLE,
  VIEW_DELETE_FAILED_TITLE,
  VIEW_PUBLISHED_TITLE,
  VIEW_UNPUBLISHED_TITLE,
} from "./SavedViewHeaderMenu.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const MEMBER = "623e4567-e89b-12d3-a456-426614174005";

function renderMenu(options: {
  view?: SavedView;
  viewerId?: string | null;
  projectRole?: number | null;
  hideCycle?: boolean;
  hideModule?: boolean;
}): { queryClient: QueryClient; view: SavedView } {
  const view = options.view ?? makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(viewKeys.projectList("acme", PROJECT), [view]);
  render(
    <QueryClientProvider client={queryClient}>
      <SavedViewHeaderMenu
        workspaceSlug="acme"
        projectId={PROJECT}
        view={view}
        viewerId={options.viewerId ?? MEMBER}
        projectRole={options.projectRole ?? 15}
        hideCycle={options.hideCycle ?? false}
        hideModule={options.hideModule ?? false}
      />
    </QueryClientProvider>
  );
  return { queryClient, view };
}

function stubClient(respond: (call: { method: string; url: string }) => { status: number; body: unknown }): Array<{
  method: string;
  url: string;
}> {
  const calls: Array<{ method: string; url: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call = { method: init.method ?? "GET", url };
    calls.push(call);
    const { status, body } = respond(call);
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return calls;
}

function cloudSlot(): {
  publish: ReturnType<typeof vi.fn>;
  unpublish: ReturnType<typeof vi.fn>;
  publicLink: (anchor: string) => string;
} {
  const slot = {
    publish: vi.fn(async (_input: unknown) => ({ anchor: "pub-1" })),
    unpublish: vi.fn(async (_input: unknown): Promise<void> => undefined),
    publicLink: (anchor: string) => `https://public.test/v/${anchor}`,
  };
  harness.edition = { id: "cloud", flags: {}, slots: { viewPublish: slot } };
  return slot;
}

async function openMenu(): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
  await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
}

async function pickMenu(label: string): Promise<void> {
  await openMenu();
  fireEvent.click(await screen.findByRole("menuitem", { name: label }));
}

afterEach(() => {
  resetAppClient();
  harness.edition = { id: "oss", flags: {} };
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
  router.navigate.mockClear();
  platformMocks.openExternal.mockClear();
  platformMocks.shareableUrl.mockClear();
  share.copyWorkItemLink.mockClear();
});

describe("SavedViewHeaderMenu standing", () => {
  it("shows edit/open/copy/delete for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: OWNER, projectRole: 15 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("shows open/copy only for a member non-owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: MEMBER, projectRole: 15 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("shows open/copy/delete for an admin non-owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: MEMBER, projectRole: 20 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("seats publish after copy on cloud for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    renderMenu({ viewerId: OWNER, projectRole: 15 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Publish",
      "Delete",
    ]);
  });

  it("seats publish for an admin non-owner on cloud", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    renderMenu({ viewerId: MEMBER, projectRole: 20 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Publish",
      "Delete",
    ]);
  });

  it("renders no publish on OSS even for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: OWNER, projectRole: 20 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).not.toContain("Publish");
  });
});

describe("SavedViewHeaderMenu menu dispatch", () => {
  it("opens the edit dialog prefilled for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Edit");
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
  });

  it("copies the canonical view address", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: MEMBER, projectRole: 15 });

    await pickMenu("Copy link");
    expect(share.copyWorkItemLink).toHaveBeenCalledWith(
      `/acme/projects/${PROJECT}/views/${VIEW_ID}`,
      expect.objectContaining({ copied: "Link Copied" })
    );
  });

  it("opens the canonical address in a new tab", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderMenu({ viewerId: MEMBER, projectRole: 15 });

    await pickMenu("Open in new tab");
    expect(platformMocks.shareableUrl).toHaveBeenCalledWith(`/acme/projects/${PROJECT}/views/${VIEW_ID}`);
    expect(platformMocks.openExternal).toHaveBeenCalledWith(
      `https://app.test/acme/projects/${PROJECT}/views/${VIEW_ID}`
    );
  });

  it("opens the publish dialog on cloud", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    renderMenu({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Publish");
    expect(await screen.findByRole("heading", { name: "Publish view" })).toBeInTheDocument();
  });
});

describe("SavedViewHeaderMenu delete", () => {
  it("confirms, removes the row, toasts and returns to the list", async () => {
    const calls = stubClient((call) =>
      call.method === "DELETE" ? { status: 204, body: "" } : { status: 200, body: [] }
    );
    const { queryClient } = renderMenu({ viewerId: OWNER, projectRole: 15 });
    const builder = await loadRichFilterBuilder();
    const detailKey = builder.savedViewKeys.project("acme", PROJECT, VIEW_ID);
    queryClient.setQueryData(detailKey, makeView({ id: VIEW_ID }));

    await pickMenu("Delete");
    expect(
      await screen.findByRole("heading", { name: "Are you sure you want to delete this view?" })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_DELETED_TITLE));
    expect(queryClient.getQueryData(detailKey)).toBeUndefined();
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/views/${VIEW_ID}/`))).toBe(true);
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/views",
      params: { ws: "acme", projectId: PROJECT },
    });
  });

  it("keeps the row and dialog when the write fails", async () => {
    stubClient((call) =>
      call.method === "DELETE" ? { status: 500, body: { detail: "boom" } } : { status: 200, body: [] }
    );
    renderMenu({ viewerId: OWNER, projectRole: 15 });
    const queryClient = new QueryClient();
    // list stays after failure: detail cache unchanged
    await pickMenu("Delete");
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(VIEW_DELETE_FAILED_TITLE, expect.any(String)));
    expect(screen.getByRole("heading", { name: "Are you sure you want to delete this view?" })).toBeInTheDocument();
    expect(router.navigate).not.toHaveBeenCalled();
    void queryClient;
  });
});

describe("SavedViewHeaderMenu publish", () => {
  it("publishes through the slot and adopts the anchor", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    const { queryClient, view } = renderMenu({ viewerId: OWNER, projectRole: 15 });
    const builder = await loadRichFilterBuilder();
    const detailKey = builder.savedViewKeys.project("acme", PROJECT, VIEW_ID);
    queryClient.setQueryData(detailKey, view);
    // open publish via menu
    await pickMenu("Publish");
    fireEvent.click(await screen.findByRole("button", { name: "Publish" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_PUBLISHED_TITLE, expect.any(String)));
    expect(slot.publish).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID });
    expect((queryClient.getQueryData(detailKey) as SavedView | undefined)?.anchor).toBe("pub-1");
  });

  it("shows link with copy and unpublish while published", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    const anchored = makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER, anchor: "pub-1" });
    const { queryClient } = renderMenu({ viewerId: OWNER, projectRole: 15, view: anchored });

    await pickMenu("Publish");
    expect(await screen.findByTestId("publish-view-link")).toHaveTextContent("https://public.test/v/pub-1");
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    expect(share.copyWorkItemLink).toHaveBeenCalledWith(
      "https://public.test/v/pub-1",
      expect.objectContaining({ copied: "Link Copied" })
    );
    fireEvent.click(screen.getByRole("button", { name: "Unpublish" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_UNPUBLISHED_TITLE, expect.any(String)));
    expect(
      (queryClient.getQueryData(viewKeys.projectList("acme", PROJECT)) as SavedView[] | undefined)?.[0]?.anchor
    ).toBeNull();
  });
});
