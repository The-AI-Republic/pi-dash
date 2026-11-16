// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Entity picker sub-pages (shell spec, SHELL-087/093).
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useCommandPalette, type PalettePage } from "../../commands/index.js";
import { PROJECT_PICKER_PAGE_ID, WORKSPACE_PICKER_PAGE_ID } from "./navigation.js";
import { projectPickerPage, workspacePickerPage } from "./navigationPages.js";

function PageProbe({ page, query }: { page: PalettePage; query: string }) {
  const state = page.usePageState(query);
  if (state.status === "loading") return <p>loading:{state.emptyText}</p>;
  return (
    <ul>
      {state.options.length === 0 ? <li>empty:{state.emptyText}</li> : null}
      {state.options.map((option) => (
        <li key={option.id}>
          <button type="button" onClick={() => void option.run()}>
            {option.title}|{option.hint}
          </button>
        </li>
      ))}
    </ul>
  );
}

afterEach(() => {
  useCommandPalette.getState().closePalette();
});

describe("project picker page", () => {
  const projects = [
    { id: "p1", name: "Website relaunch", identifier: "WEB" },
    { id: "p2", name: "Mobile app", identifier: "MOB" },
  ];

  it("lists projects and navigates on pick, closing the palette", () => {
    const onPick = vi.fn();
    useCommandPalette.getState().openPalette();
    const page = projectPickerPage({ projects, onPick });
    expect(page.id).toBe(PROJECT_PICKER_PAGE_ID);
    expect(page.placeholder).toBe("Open a project");
    render(<PageProbe page={page} query="" />);
    expect(screen.getByRole("button", { name: "Website relaunch|WEB" })).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Mobile app|MOB" }));
    expect(onPick).toHaveBeenCalledWith("p2");
    expect(useCommandPalette.getState().open).toBe(false);
  });

  it("filters by name and identifier", () => {
    const page = projectPickerPage({ projects, onPick: () => undefined });
    const { rerender } = render(<PageProbe page={page} query="mob" />);
    expect(screen.queryByRole("button", { name: "Website relaunch|WEB" })).toBeNull();
    expect(screen.getByRole("button", { name: "Mobile app|MOB" })).not.toBeNull();
    rerender(<PageProbe page={page} query="nothing" />);
    expect(screen.getByText("empty:No projects found")).not.toBeNull();
  });

  it("spins while the list resolves", () => {
    const page = projectPickerPage({ projects: undefined, onPick: () => undefined });
    render(<PageProbe page={page} query="" />);
    expect(screen.getByText("loading:No projects found")).not.toBeNull();
  });
});

describe("workspace picker page", () => {
  const workspaces = [
    { id: "w1", name: "Acme", slug: "acme" },
    { id: "w2", name: "Beta", slug: "beta" },
  ];

  it("lists workspaces and navigates on pick, closing the palette", () => {
    const onPick = vi.fn();
    useCommandPalette.getState().openPalette();
    const page = workspacePickerPage({ workspaces, onPick });
    expect(page.id).toBe(WORKSPACE_PICKER_PAGE_ID);
    expect(page.placeholder).toBe("Open a workspace");
    render(<PageProbe page={page} query="" />);
    fireEvent.click(screen.getByRole("button", { name: "Beta|beta" }));
    expect(onPick).toHaveBeenCalledWith("beta");
    expect(useCommandPalette.getState().open).toBe(false);
  });

  it("filters and spins while the list resolves", () => {
    const page = workspacePickerPage({ workspaces, onPick: () => undefined });
    const { rerender } = render(<PageProbe page={page} query="acm" />);
    expect(screen.getByRole("button", { name: "Acme|acme" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Beta|beta" })).toBeNull();
    rerender(<PageProbe page={page} query="nothing" />);
    expect(screen.getByText("empty:No workspaces found")).not.toBeNull();
    const pending = workspacePickerPage({ workspaces: undefined, onPick: () => undefined });
    rerender(<PageProbe page={pending} query="" />);
    expect(screen.getByText("loading:No workspaces found")).not.toBeNull();
  });
});
