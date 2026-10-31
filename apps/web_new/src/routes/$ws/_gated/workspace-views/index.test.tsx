// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list route (parity/specs/views.md §9, VIEW-029). The route
// plumbs its workspace param into the views-owned screen; the screen suite
// owns the states. Reads are stubbed so the route renders without a server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const session = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));
vi.mock("../../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/session/hooks.js")>()),
  useMe: session.useMe,
  usePermissions: session.usePermissions,
}));

const roles = vi.hoisted(() => ({ useProjectRoles: vi.fn() }));
vi.mock("../../../../features/auth/queries.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../features/auth/queries.js")>()),
  useProjectRoles: roles.useProjectRoles,
}));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => vi.fn(),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { makeView } from "../../../../features/views/testing.js";
import { Route } from "./index.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.includes("/api/workspaces/acme/views/"))
      return jsonResponse(url, 200, [makeView({ name: "Triage board" })]);
    if (url.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [
        { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
      ]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
  session.useMe.mockReset();
  session.usePermissions.mockReset();
  roles.useProjectRoles.mockReset();
  vi.restoreAllMocks();
});

function seedSession(): void {
  session.useMe.mockReturnValue({ data: { id: "owner-id" } });
  session.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  roles.useProjectRoles.mockReturnValue({ data: undefined });
}

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
  const Component = Route.options.component;
  if (!Component) throw new Error("missing component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Component />
    </QueryClientProvider>
  );
}

describe("workspace views list route", () => {
  it("mounts the views-owned list screen for the workspace param", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Component />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    expect(screen.getByTestId("workspace-views-customs")).toHaveTextContent("Triage board");
    expect(screen.getByTestId("workspace-views-defaults")).toHaveTextContent("All work items");
    expect(document.title).toBe("Acme - All Views");
  });

  it("opens the prefilled edit dialog from the row menu and closes it", async () => {
    seedSession();
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage board" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit View" }));
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Triage board");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Update View" })).not.toBeInTheDocument());
  });

  it("opens the delete confirmation from the row menu and closes it", async () => {
    seedSession();
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage board" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
    expect(
      await screen.findByRole("heading", { name: "Are you sure you want to delete this view?" })
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Are you sure you want to delete this view?" })
      ).not.toBeInTheDocument()
    );
  });
});
