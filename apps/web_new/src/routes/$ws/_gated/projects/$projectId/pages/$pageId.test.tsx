// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail route wiring (parity/specs/pages.md §2.6, PAGE-080): the search
// parser owns the two shareable keys, the params reach the screen, and
// every screen-side key change navigates the same address.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const router = vi.hoisted(() => ({ navigate: vi.fn(), search: {} as Record<string, unknown> }));
const screenSpy = vi.hoisted(() => ({ render: vi.fn() }));
const exportRegistration = vi.hoisted(() => ({ load: vi.fn(async () => true) }));

vi.mock("../../../../../../features/pages-export/lib/loadPageExportRegistration.js", () => ({
  loadPageExportRegistration: exportRegistration.load,
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: unknown) => ({
    options,
    useParams: () => ({ ws: "acme", projectId: "project-1", pageId: "page-1" }),
    useSearch: () => router.search,
  }),
  useNavigate: () => router.navigate,
}));

vi.mock("../../../../../../features/pages/components/PageDetailScreen.js", () => ({
  PageDetailScreen: (props: { onSearchChange: (next: unknown) => void }) => {
    screenSpy.render(props);
    return (
      <button type="button" onClick={() => props.onSearchChange({ paneTab: "info" })}>
        change keys
      </button>
    );
  },
}));

import { Route } from "./$pageId.js";

beforeEach(() => {
  exportRegistration.load.mockClear();
});

describe("page detail route", () => {
  it("parses only the two supported deep-link keys", () => {
    const validate = Route.options.validateSearch as unknown as (input: Record<string, unknown>) => unknown;
    expect(validate({})).toEqual({});
    expect(validate({ paneTab: "info", version: "v1", type: "private" })).toEqual({ paneTab: "info", version: "v1" });
    expect(validate({ paneTab: "sideways" })).toEqual({});
    expect(validate({ paneTab: "assets" })).toEqual({ paneTab: "assets" });
  });

  it("requests the export registration chunk on mount", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(exportRegistration.load).toHaveBeenCalledTimes(1);
  });

  it("passes the params and parsed search to the screen", () => {
    router.search = { paneTab: "outline" };
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByRole("button", { name: "change keys" })).toBeInTheDocument();
    expect(screenSpy.render).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceSlug: "acme",
        projectId: "project-1",
        pageId: "page-1",
        search: { paneTab: "outline" },
      })
    );
  });

  it("narrows the router-merged search before the screen sees it (PAGE-045)", () => {
    // The router merges unvalidated parent search under the child's keys,
    // so an unknown tab arrives raw despite validateSearch; the screen
    // must receive the narrowed form or the pane opens on presence alone.
    router.search = { paneTab: "zzz", version: "v1", type: "private" };
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screenSpy.render).toHaveBeenCalledWith(
      expect.objectContaining({
        search: { version: "v1" },
      })
    );
  });

  it("writes key changes back into the same address", () => {
    router.navigate.mockReset();
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    screen.getByRole("button", { name: "change keys" }).click();
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/pages/$pageId",
      params: { ws: "acme", projectId: "project-1", pageId: "page-1" },
      search: { paneTab: "info" },
      replace: true,
    });
  });
});
