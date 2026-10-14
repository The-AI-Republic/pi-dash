// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { setSessionClient } from "../../../core/session/queries.js";
import { Route } from "./index.js";

// The real router except links: project rows render plain anchors.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({ children }: { children?: React.ReactNode }) => <a href="#">{children}</a>,
  };
});

function HomeComponent(): React.ReactElement {
  const Component = Route.options.component as unknown as React.ComponentType;
  return <Component />;
}

function renderHome(
  tourCompleted: boolean,
  projects: ReadonlyArray<{ id: string; name: string }> = []
): void {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  vi.spyOn(Route, "useLoaderData").mockReturnValue({ projects } as never);
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "profile"], { is_tour_completed: tourCompleted });
  queryClient.setQueryData(["session", "me"], { first_name: "Ada" });
  render(
    <QueryClientProvider client={queryClient}>
      <HomeComponent />
    </QueryClientProvider>
  );
  vi.restoreAllMocks();
}

describe("workspace home tour mount", () => {
  it("shows the tour overlay over home while the flag is false", () => {
    renderHome(false);
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash, Ada" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No projects yet" })).toBeInTheDocument();
  });

  it("shows home alone once the flag is set", () => {
    renderHome(true);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No projects yet" })).toBeInTheDocument();
  });
});

describe("workspace home interim project list", () => {
  it("lists project links instead of forwarding while the dashboard is pending", () => {
    renderHome(true, [
      { id: "p1", name: "Alpha" },
      { id: "p2", name: "Beta" },
    ]);
    expect(screen.getByRole("heading", { name: "Projects" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Alpha" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Beta" })).toBeInTheDocument();
  });
});
