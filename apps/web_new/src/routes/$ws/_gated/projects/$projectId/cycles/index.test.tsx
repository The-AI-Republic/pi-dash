// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles tab home. Thin mount: proves the route renders the cycles
// list shell with the list body for the addressed project, validates
// the ?peekCycle= address key, and writes the key back on peek changes.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

vi.mock("../../../../../../features/cycles/components/CyclesListScreen.js", () => ({
  CyclesListScreen: (props: Record<string, unknown>) => (
    <div
      data-testid="cycles-screen"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
    >
      {props.children as never}
    </div>
  ),
}));

vi.mock("../../../../../../features/cycles/components/CyclesListBody.js", () => ({
  CyclesListBody: (props: Record<string, unknown>) => (
    <div
      data-testid="cycles-body"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
      data-peek={String(props.peekCycleId ?? "")}
    >
      <button type="button" onClick={() => (props.onPeekChange as (cycleId: string | null) => void)("cycle-1")}>
        open
      </button>
      <button type="button" onClick={() => (props.onPeekChange as (cycleId: string | null) => void)(null)}>
        close
      </button>
    </div>
  ),
}));

import { parseCyclesListSearch } from "../../../../../../features/cycles/lib/cycleListSearch.js";
import { Route } from "./index.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function renderRoute(search: Record<string, unknown>): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT } as never);
  vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
  const Component = Route.options.component;
  if (!Component) throw new Error("missing component");
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Component />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  mocks.navigate.mockReset();
});

describe("cycles tab route", () => {
  it("renders the shell with the list body for the project", () => {
    renderRoute({});
    const node = screen.getByTestId("cycles-screen");
    expect(node).toHaveAttribute("data-workspace", "acme");
    expect(node).toHaveAttribute("data-project", PROJECT);
    const body = screen.getByTestId("cycles-body");
    expect(body).toHaveAttribute("data-workspace", "acme");
    expect(body).toHaveAttribute("data-project", PROJECT);
    expect(body).toHaveAttribute("data-peek", "");
  });

  it("passes the peek key from the address into the body", () => {
    renderRoute({ peekCycle: "cycle-9" });
    expect(screen.getByTestId("cycles-body")).toHaveAttribute("data-peek", "cycle-9");
  });

  it("validates search through the list search parser", () => {
    const validateSearch = Route.options.validateSearch as unknown as (input: Record<string, unknown>) => unknown;
    expect(validateSearch({})).toEqual(parseCyclesListSearch({}));
    expect(validateSearch({ peekCycle: "cycle-9" })).toEqual(parseCyclesListSearch({ peekCycle: "cycle-9" }));
  });

  it("writes the peek key back to the address on peek changes", () => {
    renderRoute({ peekCycle: "cycle-1" });
    fireEvent.click(screen.getByRole("button", { name: "open" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { peekCycle: "cycle-1" },
      replace: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "close" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({ to: ".", search: {}, replace: true });
  });
});
