// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules tab home address (parity/specs/modules.md §5, MOD-019). The
// route validates the search down to its `peekModule` key, hands the
// screen the module it names, and writes the key back through the router —
// opening replaces the entry rather than pushing one, and closing removes
// only that key. The screen becomes a probe here so the suite owns only the
// address plumbing; the list screen's own suite covers its states.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const MODULE_ID = "323e4567-e89b-12d3-a456-426614174002";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn(), search: {} as Record<string, unknown> }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

vi.mock("../../../../../../features/modules/components/ModulesListScreen.js", () => ({
  ModulesListScreen: (props: {
    workspaceSlug: string;
    projectId: string;
    peekModuleId?: string | null;
    onPeekChange?: (moduleId: string | null) => void;
  }) => (
    <div data-testid="screen-stub" data-workspace={props.workspaceSlug} data-project={props.projectId}>
      <span data-testid="screen-peek">{props.peekModuleId ?? ""}</span>
      <button
        type="button"
        onClick={() => {
          props.onPeekChange?.(MODULE_ID);
        }}
      >
        Open peek
      </button>
      <button
        type="button"
        onClick={() => {
          props.onPeekChange?.(null);
        }}
      >
        Close peek
      </button>
    </div>
  ),
}));

import { Route } from "./index.js";

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID } as never);
  vi.spyOn(Route, "useSearch").mockReturnValue(routerHarness.search as never);
  const Component = Route.options.component;
  if (!Component) throw new Error("missing component");
  render(
    <React.Suspense fallback={null}>
      <Component />
    </React.Suspense>
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  routerHarness.navigate.mockReset();
  routerHarness.search = {};
});

describe("modules tab route search", () => {
  it("validates peekModule and keeps the other address keys", () => {
    const validateSearch = Route.options.validateSearch as unknown as (raw: Record<string, unknown>) => unknown;
    expect(validateSearch({})).toEqual({});
    expect(validateSearch({ peekModule: MODULE_ID })).toEqual({ peekModule: MODULE_ID });
    expect(validateSearch({ peekModule: "not-an-id" })).toEqual({});
    expect(validateSearch({ peekModule: MODULE_ID, layout: "board" })).toEqual({
      peekModule: MODULE_ID,
      layout: "board",
    });
    expect(validateSearch({ peekModule: "not-an-id", layout: "board" })).toEqual({ layout: "board" });
  });

  it("plumbs the params into the screen", async () => {
    renderRoute();
    expect(await screen.findByTestId("screen-stub")).toHaveAttribute("data-workspace", "acme");
    expect(screen.getByTestId("screen-stub")).toHaveAttribute("data-project", PROJECT_ID);
    expect(screen.getByTestId("screen-peek")).toBeEmptyDOMElement();
  });

  it("hands the screen the peeked module from the address", async () => {
    routerHarness.search = { peekModule: MODULE_ID };
    renderRoute();
    expect(await screen.findByTestId("screen-peek")).toHaveTextContent(MODULE_ID);
  });

  it("opens the peek by writing the key and replacing the entry", async () => {
    routerHarness.search = { layout: "board" };
    renderRoute();
    fireEvent.click(await screen.findByRole("button", { name: "Open peek" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: ".",
      search: { layout: "board", peekModule: MODULE_ID },
      replace: true,
    });
  });

  it("closes the peek by removing only that key", async () => {
    routerHarness.search = { peekModule: MODULE_ID, layout: "board" };
    renderRoute();
    fireEvent.click(await screen.findByRole("button", { name: "Close peek" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: ".", search: { layout: "board" }, replace: true });
  });
});
