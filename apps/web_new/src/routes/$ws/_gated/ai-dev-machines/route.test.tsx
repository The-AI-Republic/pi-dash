// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI dev machines layout (NEWFRONT-540, RUN-001). The area shares the
// runners workspace rule: the role record is awaited, members and admins
// reach the outlet, guests get the area's not-authorized view.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, Outlet: () => <div data-testid="outlet" /> };
});

const { createAppClient, resetAppClient } = await import("../../../../core/api/client.js");
const { sessionKeys, setSessionClient } = await import("../../../../core/session/index.js");
const { Route } = await import("./route.js");

function mount(role: number | null): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
  setSessionClient({} as never);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  if (role !== null) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "ws-id", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role } satisfies Workspace,
    ]);
  }
  const Component = Route.options.component;
  if (!Component) throw new Error("ai-dev-machines layout has no component");
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("ai dev machines layout", () => {
  it("keeps guests out with the area's not-authorized view", () => {
    mount(5);
    expect(screen.getByRole("heading", { name: "Oops! You are not authorized to view this page" })).toBeInTheDocument();
    expect(screen.queryByTestId("outlet")).toBeNull();
  });

  it("keeps callers without a membership out", () => {
    mount(null);
    expect(screen.queryByTestId("outlet")).toBeNull();
  });

  it("admits members and admins", () => {
    mount(15);
    expect(screen.getByTestId("outlet")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /not authorized/i })).toBeNull();
  });

  it("admits admins", () => {
    mount(20);
    expect(screen.getByTestId("outlet")).toBeInTheDocument();
  });
});
