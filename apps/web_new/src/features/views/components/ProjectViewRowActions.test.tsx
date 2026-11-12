// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SavedView } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

const session = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));
vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  useMe: session.useMe,
  usePermissions: session.usePermissions,
}));

const roles = vi.hoisted(() => ({ useProjectRoles: vi.fn() }));
vi.mock("../../auth/queries.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../auth/queries.js")>()),
  useProjectRoles: roles.useProjectRoles,
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
  ProjectViewRowActions,
  VIEW_DELETED_TITLE,
  VIEW_DELETE_FAILED_TITLE,
  VIEW_PUBLISHED_TITLE,
  VIEW_PUBLISH_FAILED_TITLE,
  VIEW_UNPUBLISHED_TITLE,
  VIEW_UNPUBLISH_FAILED_TITLE,
} from "./ProjectViewRowActions.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const VIEW_ID = "223e4567-e89b-12d3-a456-426614174001";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const MEMBER = "623e4567-e89b-12d3-a456-426614174005";
const WORKSPACE_ID = "123e4567-e89b-12d3-a456-426614174000";
const FAVORITE_ID = "723e4567-e89b-12d3-a456-426614174006";

function seedSession(): void {
  session.useMe.mockReturnValue({ data: { id: OWNER } });
  session.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  roles.useProjectRoles.mockReturnValue({ data: undefined });
}

interface Call {
  method: string;
  url: string;
}

function stubClient(respond: (call: Call) => { status: number; body: unknown }): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call: Call = { method: init.method ?? "GET", url };
    calls.push(call);
    const { status, body } = respond(call);
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return calls;
}

function favoriteBody(): Record<string, unknown> {
  return {
    id: FAVORITE_ID,
    entity_type: "view",
    entity_identifier: VIEW_ID,
    entity_data: null,
    name: "Triage",
    is_folder: false,
    sequence: 10000,
    parent: null,
    workspace_id: WORKSPACE_ID,
    project_id: PROJECT,
  };
}

interface RenderActionsOptions {
  view?: SavedView;
  viewerId?: string | null;
  projectRole?: number | null;
  publishOpen?: boolean;
  onPublishOpenChange?: (open: boolean) => void;
  seedList?: boolean;
}

function renderActions(options: RenderActionsOptions = {}): {
  queryClient: QueryClient;
  onPublishOpenChange: Mock<(open: boolean) => void>;
  view: SavedView;
} {
  seedSession();
  const view = options.view ?? makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedList ?? true) {
    queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT), [view]);
  }
  const onPublishOpenChange = (options.onPublishOpenChange ?? vi.fn()) as Mock<(open: boolean) => void>;
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectViewRowActions
        workspaceSlug="acme"
        projectId={PROJECT}
        view={view}
        viewerId={options.viewerId ?? MEMBER}
        projectRole={options.projectRole ?? 15}
        hideCycle={false}
        hideModule={false}
        publishOpen={options.publishOpen ?? false}
        onPublishOpenChange={onPublishOpenChange}
      />
    </QueryClientProvider>
  );
  return { queryClient, onPublishOpenChange, view };
}

function listRows(queryClient: QueryClient): SavedView[] {
  return queryClient.getQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT)) ?? [];
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
  session.useMe.mockReset();
  session.usePermissions.mockReset();
  roles.useProjectRoles.mockReset();
  globalThis.localStorage.clear();
});

describe("ProjectViewRowActions standing", () => {
  it("hides the star for guests but keeps open and copy", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: MEMBER, projectRole: 5 });

    expect(screen.queryByTestId("project-views-row-star")).toBeNull();
    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("shows the star for members with open and copy only", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: MEMBER, projectRole: 15 });

    expect(screen.getByTestId("project-views-row-star")).toBeInTheDocument();
    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("adds delete for admins", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: MEMBER, projectRole: 20 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("adds edit and delete for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: OWNER, projectRole: 15 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
  });

  it("seats publish after copy on cloud for the owner or an admin", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    renderActions({ viewerId: OWNER, projectRole: 15 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit",
      "Open in new tab",
      "Copy link",
      "Publish",
      "Delete",
    ]);
  });

  it("renders no publish entry on OSS even for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: OWNER, projectRole: 20 });

    await openMenu();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).not.toContain("Publish");
  });
});

describe("ProjectViewRowActions star", () => {
  it("toggles the favorite silently", async () => {
    const calls = stubClient((call) =>
      call.method === "POST" ? { status: 201, body: favoriteBody() } : { status: 200, body: [] }
    );
    const { queryClient } = renderActions({ viewerId: OWNER, projectRole: 15 });

    fireEvent.click(screen.getByTestId("project-views-row-star"));
    await waitFor(() => expect(listRows(queryClient)[0]?.is_favorite).toBe(true));
    expect(calls.some((call) => call.method === "POST" && call.url.includes("/user-favorites/"))).toBe(true);
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
  });
});

describe("ProjectViewRowActions menu dispatch", () => {
  it("opens the edit dialog prefilled for the owner", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Edit");
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Triage");
  });

  it("copies the absolute deep link", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: MEMBER, projectRole: 15 });

    await pickMenu("Copy link");
    expect(share.copyWorkItemLink).toHaveBeenCalledWith(
      `/acme/projects/${PROJECT}/views/${VIEW_ID}`,
      expect.objectContaining({ copied: "Link Copied" })
    );
  });

  it("opens the same address in a new tab", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    renderActions({ viewerId: MEMBER, projectRole: 15 });

    await pickMenu("Open in new tab");
    expect(platformMocks.shareableUrl).toHaveBeenCalledWith(`/acme/projects/${PROJECT}/views/${VIEW_ID}`);
    expect(platformMocks.openExternal).toHaveBeenCalledWith(
      `https://app.test/acme/projects/${PROJECT}/views/${VIEW_ID}`
    );
  });

  it("asks the row to open the publish dialog", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    const { onPublishOpenChange } = renderActions({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Publish");
    expect(onPublishOpenChange).toHaveBeenCalledWith(true);
  });
});

describe("ProjectViewRowActions delete", () => {
  it("confirms, removes the row, toasts and returns to the list", async () => {
    const calls = stubClient((call) =>
      call.method === "DELETE" ? { status: 204, body: "" } : { status: 200, body: [] }
    );
    const { queryClient } = renderActions({ viewerId: OWNER, projectRole: 15 });
    const builder = await loadRichFilterBuilder();
    const detailKey = builder.savedViewKeys.project("acme", PROJECT, VIEW_ID);
    queryClient.setQueryData(detailKey, makeView({ id: VIEW_ID }));

    await pickMenu("Delete");
    expect(
      await screen.findByRole("heading", { name: "Are you sure you want to delete this view?" })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_DELETED_TITLE));
    expect(listRows(queryClient)).toEqual([]);
    expect(queryClient.getQueryData(detailKey)).toBeUndefined();
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/views/${VIEW_ID}/`))).toBe(true);
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/views",
      params: { ws: "acme", projectId: PROJECT },
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Are you sure you want to delete this view?" })
      ).not.toBeInTheDocument()
    );
  });

  it("keeps the row and the open dialog when the write fails", async () => {
    stubClient((call) =>
      call.method === "DELETE" ? { status: 500, body: { detail: "boom" } } : { status: 200, body: [] }
    );
    const { queryClient, view } = renderActions({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Delete");
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(VIEW_DELETE_FAILED_TITLE, expect.any(String)));
    expect(listRows(queryClient).map((row) => row.id)).toEqual([view.id]);
    expect(screen.getByRole("heading", { name: "Are you sure you want to delete this view?" })).toBeInTheDocument();
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("cancels without writing", async () => {
    const calls = stubClient(() => ({ status: 200, body: {} }));
    const { queryClient, view } = renderActions({ viewerId: OWNER, projectRole: 15 });

    await pickMenu("Delete");
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Are you sure you want to delete this view?" })
      ).not.toBeInTheDocument()
    );
    expect(listRows(queryClient).map((row) => row.id)).toEqual([view.id]);
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
  });
});

describe("ProjectViewRowActions publish", () => {
  it("publishes through the slot and adopts the anchor", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    const { queryClient, view } = renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true });
    const builder = await loadRichFilterBuilder();
    const detailKey = builder.savedViewKeys.project("acme", PROJECT, VIEW_ID);
    queryClient.setQueryData(detailKey, view);

    fireEvent.click(await screen.findByRole("button", { name: "Publish" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_PUBLISHED_TITLE, expect.any(String)));
    expect(slot.publish).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID });
    expect(listRows(queryClient)[0]?.anchor).toBe("pub-1");
    expect((queryClient.getQueryData(detailKey) as SavedView | undefined)?.anchor).toBe("pub-1");
  });

  it("shows the link with copy and unpublish while published", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    const anchored = makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER, anchor: "pub-1" });
    const { queryClient } = renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true, view: anchored });

    expect(await screen.findByTestId("publish-view-link")).toHaveTextContent("https://public.test/v/pub-1");
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    expect(share.copyWorkItemLink).toHaveBeenCalledWith(
      "https://public.test/v/pub-1",
      expect.objectContaining({ copied: "Link Copied" })
    );
    fireEvent.click(screen.getByRole("button", { name: "Unpublish" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith(VIEW_UNPUBLISHED_TITLE, expect.any(String)));
    expect(slot.unpublish).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID });
    expect(listRows(queryClient)[0]?.anchor).toBeNull();
  });

  it("keeps the dialog open when publishing fails", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    slot.publish.mockRejectedValueOnce(new Error("cloud is down"));
    const { queryClient } = renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true });

    fireEvent.click(await screen.findByRole("button", { name: "Publish" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(VIEW_PUBLISH_FAILED_TITLE, expect.any(String)));
    expect(listRows(queryClient)[0]?.anchor).toBeUndefined();
    expect(screen.getByRole("heading", { name: "Publish view" })).toBeInTheDocument();
  });

  it("keeps the anchor when unpublishing fails", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    slot.unpublish.mockRejectedValueOnce(new Error("cloud is down"));
    const anchored = makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER, anchor: "pub-1" });
    const { queryClient } = renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true, view: anchored });

    fireEvent.click(await screen.findByRole("button", { name: "Unpublish" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(VIEW_UNPUBLISH_FAILED_TITLE, expect.any(String))
    );
    expect(listRows(queryClient)[0]?.anchor).toBe("pub-1");
    expect(screen.getByRole("heading", { name: "Published view" })).toBeInTheDocument();
  });

  it("closes the publish dialog from its cancel", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    cloudSlot();
    const { onPublishOpenChange } = renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true });

    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(onPublishOpenChange).toHaveBeenCalledWith(false);
  });

  it("does nothing when the slot disappears before the write", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true });

    harness.edition = { id: "oss", flags: {} };
    fireEvent.click(await screen.findByRole("button", { name: "Publish" }));
    await waitFor(() => expect(slot.publish).not.toHaveBeenCalled());
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
  });

  it("does nothing when the slot disappears before unpublishing", async () => {
    stubClient(() => ({ status: 200, body: {} }));
    const slot = cloudSlot();
    const anchored = makeView({ id: VIEW_ID, project: PROJECT, owned_by: OWNER, anchor: "pub-1" });
    renderActions({ viewerId: OWNER, projectRole: 15, publishOpen: true, view: anchored });

    harness.edition = { id: "oss", flags: {} };
    fireEvent.click(await screen.findByRole("button", { name: "Unpublish" }));
    await waitFor(() => expect(slot.unpublish).not.toHaveBeenCalled());
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
  });
});
