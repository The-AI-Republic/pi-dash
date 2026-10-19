// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General page container (WSET-003–006): loading skeleton, admin vs
// member composition, the copy control (confirmation and silent refusal),
// the resolved logo URL and the post-delete landing target.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getSessionClient, settingsQueryOptions, workspacesQueryOptions } from "../../../core/session/index.js";
import { copyWorkspaceAddress, WorkspaceGeneralPage } from "./WorkspaceGeneralPage.js";
import { workspaceDetailsKey } from "./api/details.js";
import { URL_COPIED_TITLE } from "./api/mutations.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  resolveFileUrl: (path: string | null | undefined, base: string) =>
    path === null || path === undefined || path === "" ? null : `${base}${path}`,
}));

import { notifySuccess } from "../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function workspaceRow(overrides: Record<string, unknown> = {}) {
  return {
    id: UUID,
    name: "Acme",
    slug: "acme",
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: UUID,
    organization_size: "11-50",
    timezone: "UTC",
    background_color: "#fff",
    total_members: 2,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    ...overrides,
  };
}

interface StubOptions {
  hangDetails?: boolean;
  deleteOk?: boolean;
  logoUrl?: string | null;
  workspaces?: Array<{ slug: string; role: number }>;
  lastSlug?: string | null;
  fallbackSlug?: string | null;
}

function stubApi(options: StubOptions = {}) {
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith("/api/workspaces/acme/") && method === "DELETE") {
      return jsonResponse(url, options.deleteOk === true ? 204 : 403, {});
    }
    if (url.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(
        url,
        200,
        (options.workspaces ?? [{ slug: "acme", role: 20 }]).map((entry) => ({
          id: UUID,
          name: entry.slug,
          slug: entry.slug,
          logo_url: null,
          total_members: 1,
          role: entry.role,
        }))
      );
    }
    if (url.endsWith("/api/users/me/settings/")) {
      return jsonResponse(url, 200, {
        id: UUID,
        email: "ann@example.com",
        workspace: {
          last_workspace_id: null,
          last_workspace_slug: options.lastSlug ?? null,
          fallback_workspace_id: null,
          fallback_workspace_slug: options.fallbackSlug ?? null,
          invites: 0,
        },
      });
    }
    if (url.endsWith("/api/timezones/")) return jsonResponse(url, 200, { timezones: [] });
    if (url.endsWith("/api/workspaces/acme/")) {
      if (options.hangDetails === true) return new Promise<never>(() => undefined);
      return jsonResponse(url, 200, workspaceRow({ logo_url: options.logoUrl ?? null }));
    }
    throw new Error(`unexpected fetch ${url}`);
  }) as never);
}

function renderPage(queryClient: QueryClient, props: Partial<React.ComponentProps<typeof WorkspaceGeneralPage>> = {}) {
  const onNavigate = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceGeneralPage workspaceSlug="acme" onNavigate={onNavigate} {...props} />
    </QueryClientProvider>
  );
  return { onNavigate };
}

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

/** Members come from the session query? No — the page reads usePermissions. */
beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("copyWorkspaceAddress", () => {
  it("confirms after a successful write", async () => {
    const confirm = vi.fn();
    await copyWorkspaceAddress("https://app.test/acme", { writeText: async () => undefined }, confirm);
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("stays silent when the clipboard refuses or is missing", async () => {
    const confirm = vi.fn();
    await copyWorkspaceAddress(
      "https://app.test/acme",
      {
        writeText: async () => {
          throw new Error("denied");
        },
      },
      confirm
    );
    await copyWorkspaceAddress("https://app.test/acme", undefined, confirm);
    expect(confirm).not.toHaveBeenCalled();
  });
});

describe("WorkspaceGeneralPage", () => {
  it("shows a loading skeleton until the details resolve", async () => {
    stubApi({ hangDetails: true });
    renderPage(testClient());
    expect(screen.getByTestId("workspace-general-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-general-identity")).not.toBeInTheDocument();
  });

  it("renders identity, form and the delete section for admins", async () => {
    stubApi();
    renderPage(testClient());
    expect(await screen.findByTestId("workspace-general-identity")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-general-form")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-delete-section")).toBeInTheDocument();
    expect(screen.getByLabelText("Workspace name")).toHaveValue("Acme");
  });

  it("dims the page for members: no delete section and no logo action", async () => {
    stubApi({ workspaces: [{ slug: "acme", role: 15 }] });
    renderPage(testClient());
    expect(await screen.findByTestId("workspace-general-identity")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-delete-section")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Upload logo" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Workspace name")).toBeDisabled();
  });

  it("resolves the stored logo into the image and offers Edit logo", async () => {
    stubApi({ logoUrl: "/api/assets/v2/static/a1/" });
    renderPage(testClient());
    expect(await screen.findByTestId("workspace-general-logo-image")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/a1/"
    );
    expect(screen.getByRole("button", { name: "Edit logo" })).toBeInTheDocument();
  });

  it("copies the address and confirms", async () => {
    stubApi();
    renderPage(testClient());
    await screen.findByTestId("workspace-general-identity");
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    await user.click(screen.getByRole("button", { name: /acme$/ }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/acme`);
    });
    expect(notifySuccess).toHaveBeenCalledWith(URL_COPIED_TITLE);
    expect(URL_COPIED_TITLE).toBe("Workspace URL copied to the clipboard.");
  });

  it("stays silent when the clipboard refuses", async () => {
    stubApi();
    renderPage(testClient());
    await screen.findByTestId("workspace-general-identity");
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    await user.click(screen.getByRole("button", { name: /acme$/ }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalled();
    });
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("keeps the dialog when the delete fails", async () => {
    stubApi({
      workspaces: [
        { slug: "beta", role: 20 },
        { slug: "acme", role: 20 },
      ],
      lastSlug: "beta",
    });
    const queryClient = testClient();
    await queryClient.prefetchQuery(settingsQueryOptions(getSessionClient()));
    await queryClient.prefetchQuery(workspacesQueryOptions(getSessionClient()));
    const { onNavigate } = renderPage(queryClient);
    await screen.findByTestId("workspace-delete-section");
    await waitFor(() => {
      expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toBeDefined();
    });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    // The stub answers 204 to the GET branch only; the DELETE is unhandled,
    // so the write fails and the page must stay put.
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => {
      expect(screen.getByLabelText(/Type the workspace name/)).toHaveValue("Acme");
    });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("lands on the last surviving workspace after a successful delete", async () => {
    stubApi({
      workspaces: [
        { slug: "beta", role: 20 },
        { slug: "acme", role: 20 },
      ],
      lastSlug: "beta",
      deleteOk: true,
    });
    const queryClient = testClient();
    await queryClient.fetchQuery(settingsQueryOptions(getSessionClient()));
    await queryClient.fetchQuery(workspacesQueryOptions(getSessionClient()));
    const { onNavigate } = renderPage(queryClient);
    await screen.findByTestId("workspace-delete-section");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => {
      expect(onNavigate).toHaveBeenCalledWith("/beta/");
    });
  });
});
