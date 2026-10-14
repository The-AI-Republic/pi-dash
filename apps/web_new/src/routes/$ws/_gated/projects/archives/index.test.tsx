// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import { sessionKeys } from "../../../../../core/session/index.js";
import { projectKeys } from "../../../../../features/projects/index.js";
import { Route } from "./index.js";

function renderArchives(): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
  const Component = Route.options.component;
  if (!Component) throw new Error("archives route has no component");
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(projectKeys.details("acme"), []);
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

describe("archived projects route", () => {
  it("mounts the archive view for the workspace slug", () => {
    renderArchives();

    expect(screen.getByLabelText("Archived projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "No projects archived" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Project/ })).toBeNull();
  });
});
