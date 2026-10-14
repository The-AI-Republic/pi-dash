// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails, Workspace } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the route wiring.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
      <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
    ),
  };
});

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/index.js";
import { projectKeys } from "../../../../features/projects/index.js";
import { Route } from "./index.js";

function renderList(): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
  const Component = Route.options.component;
  if (!Component) throw new Error("projects route has no component");
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(projectKeys.details("acme"), [
    { id: "223e4567-e89b-12d3-a456-426614174001", name: "Alpha", archived_at: null } as ProjectDetails,
  ]);
  queryClient.setQueryData(sessionKeys.workspaces, [
    {
      id: "123e4567-e89b-12d3-a456-426614174000",
      name: "Acme",
      slug: "acme",
      logo_url: null,
      total_members: 2,
      role: 20,
    } satisfies Workspace,
  ]);
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("projects list route", () => {
  it("mounts the connected list for the workspace slug", () => {
    renderList();

    expect(screen.getByLabelText("Projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Alpha" })).not.toBeNull();
    expect(screen.getByRole("button", { name: /Project/ })).not.toBeNull();
  });
});
