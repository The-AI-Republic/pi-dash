// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { setSessionClient } from "../../../core/session/queries.js";
import { Route } from "./index.js";

function HomeComponent(): React.ReactElement {
  const Component = Route.options.component as unknown as React.ComponentType;
  return <Component />;
}

function renderHome(tourCompleted: boolean): void {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "profile"], { is_tour_completed: tourCompleted });
  queryClient.setQueryData(["session", "me"], { first_name: "Ada" });
  render(
    <QueryClientProvider client={queryClient}>
      <HomeComponent />
    </QueryClientProvider>
  );
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
