// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../core/api/index.js";
import {
  clearCommands,
  clearPalettePages,
  registerCommands,
  registerPalettePages,
  useCommandPalette,
  type PalettePageState,
  type PaletteSearchScope,
  type SearchResultHit,
} from "../commands/index.js";
import { TOPBAR_SEARCH_PLACEHOLDER, TopBarSearch } from "./TopBarSearch.js";
import { useTopSearchFocus } from "./topSearchFocus.js";

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const ISSUE_ID = "22222222-2222-4222-8222-222222222222";
const SCOPE: PaletteSearchScope = { workspaceSlug: "acme", projectId: PROJECT_ID };

const ISSUE_RESULTS = {
  results: {
    issue: [
      {
        name: "Fix login",
        id: ISSUE_ID,
        sequence_id: 12,
        project__identifier: "ATL",
        project_id: PROJECT_ID,
        workspace__slug: "acme",
      },
    ],
  },
};

function renderBox(
  options: {
    scope?: PaletteSearchScope | null;
    onSelectSearchResult?: (hit: SearchResultHit) => void;
    search?: (url: string) => unknown | Promise<unknown>;
  } = {}
): { calls: string[] } {
  const calls: string[] = [];
  createAppClient("https://api.test", async (url): Promise<TransportResponse> => {
    calls.push(url);
    const body = options.search ? await options.search(url) : { results: {} };
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <TopBarSearch
        scope={options.scope === undefined ? SCOPE : options.scope}
        onSelectSearchResult={options.onSelectSearchResult ?? (() => undefined)}
      />
    </QueryClientProvider>
  );
  return { calls };
}

function searchParams(url: string): Record<string, string> {
  return Object.fromEntries(new URL(url).searchParams.entries());
}

afterEach(() => {
  // No resetAppClient: pending query updates can re-render after this
  // hook, and every render configures a fresh client up front.
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
  useTopSearchFocus.setState({ request: 0 });
});

// The search rows live in a lazily imported panel; the first import can
// take seconds while the whole suite competes for the CPU.
const LAZY_ROW_TIMEOUT = { timeout: 10_000 };

describe("TopBarSearch (SHELL-081)", () => {
  it("focuses and opens the panel on a focus request (SHELL-089)", () => {
    renderBox({ scope: null });
    const field = screen.getByLabelText("Search commands");
    expect(document.activeElement).not.toBe(field);
    expect(screen.queryByRole("listbox", { name: "Top bar search results" })).toBeNull();
    act(() => {
      useTopSearchFocus.getState().requestTopSearchFocus();
    });
    expect(document.activeElement).toBe(field);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).not.toBeNull();
  });

  it("never steals focus on mount, even under StrictMode replays (SHELL-089)", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <React.StrictMode>
        <QueryClientProvider client={queryClient}>
          <TopBarSearch scope={null} onSelectSearchResult={() => undefined} />
        </QueryClientProvider>
      </React.StrictMode>
    );
    const field = screen.getByLabelText("Search commands");
    expect(document.activeElement).not.toBe(field);
    expect(screen.queryByRole("listbox", { name: "Top bar search results" })).toBeNull();
    act(() => {
      useTopSearchFocus.getState().requestTopSearchFocus();
    });
    expect(document.activeElement).toBe(field);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).not.toBeNull();
  });

  it("starts closed and opens an inline panel on focus", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "shell.sign-out", title: "Sign out", run: () => undefined },
      { id: "shell.go-home", title: "Go to workspace home", run: () => undefined },
    ]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER);
    expect(input).toHaveAttribute("placeholder", "Search commands...");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.click(input);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // The rows live in a lazily imported panel; under a full-suite run
    // that chunk lands well after the 1 s default find budget.
    expect(await screen.findByRole("option", { name: "Sign out" }, LAZY_ROW_TIMEOUT)).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: "Go to workspace home" }, LAZY_ROW_TIMEOUT)).toBeInTheDocument();
  });

  it("filters commands as the term types", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "theme", title: "Change interface theme", run: () => undefined },
      { id: "shell.sign-out", title: "Sign out", run: () => undefined },
    ]);
    renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "interface theme");
    expect(await screen.findByRole("option", { name: "Change interface theme" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Sign out" })).not.toBeInTheDocument();
  });

  it("clears the term and closes on a single Escape", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER) as HTMLInputElement;
    await user.click(input);
    await user.type(input, "interface theme");
    expect(input.value).toBe("interface theme");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(input.value).toBe("");
  });

  it("reopens when the focused box is clicked again (SHELL-081)", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER);
    await user.click(input);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(input);
    // Focus never left, so only the click reopens.
    await user.click(input);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).toBeInTheDocument();
  });

  it("closes on Escape with an empty term", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("closes and resets the term on an outside click", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER) as HTMLInputElement;
    await user.click(input);
    await user.type(input, "abc");
    expect(input.value).toBe("abc");
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(input.value).toBe("");
  });

  it("keeps the panel open for clicks inside it", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER);
    await user.click(input);
    fireEvent.pointerDown(input);
    expect(screen.getByRole("listbox", { name: "Top bar search results" })).toBeInTheDocument();
  });

  it("runs a command and closes", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([{ id: "theme", title: "Change interface theme", run }]);
    renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.click(await screen.findByRole("option", { name: "Change interface theme" }));
    expect(run).toHaveBeenCalledOnce();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("moves with arrows and activates on Enter", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([
      { id: "a", title: "Alpha", run: () => undefined },
      { id: "b", title: "Beta", run },
    ]);
    renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await screen.findByRole("option", { name: "Beta" });
    await user.keyboard("{ArrowDown}");
    expect(document.querySelector('[role="option"][aria-selected="true"]')).toHaveTextContent("Beta");
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("hands page commands to the modal palette", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "project.open", title: "Open a project", page: "test.project-picker", run: () => undefined },
    ]);
    registerPalettePages([
      {
        id: "test.project-picker",
        placeholder: "Search projects",
        usePageState: (): PalettePageState => ({ status: "ready", options: [], emptyText: "None" }),
      },
    ]);
    renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.click(await screen.findByRole("option", { name: "Open a project" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(useCommandPalette.getState().open).toBe(true);
    expect(useCommandPalette.getState().page).toBe("test.project-picker");
  });

  it("resets when the modal palette opens", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "theme", title: "Change interface theme", run: () => undefined }]);
    renderBox({ scope: null });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER) as HTMLInputElement;
    await user.click(input);
    await user.type(input, "abc");
    act(() => {
      useCommandPalette.getState().openPalette();
    });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(input.value).toBe("");
  });
});

describe("TopBarSearch server results (SHELL-081/084/085/093)", () => {
  it("shows grouped hits for a typed term, project-scoped", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const { calls } = renderBox({ search: () => ISSUE_RESULTS });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "Parity");
    expect(await screen.findByText(/Search results for/, undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 })).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(1);
    expect(searchParams(calls[0]!)).toEqual({ search: "Parity", project_id: PROJECT_ID, workspace_search: "false" });
  });

  it("pulses the heading and spins while the search flies", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderBox({ search: () => gate.then(() => ISSUE_RESULTS) });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "Parity");
    const heading = await screen.findByText(/Search results for/, undefined, { timeout: 5000 });
    expect(heading.className).toContain("animate-pulse");
    expect(screen.getByLabelText("Server results")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Searching")).toBeInTheDocument();
    release();
    expect(await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 })).toBeInTheDocument();
    expect(heading.className).not.toContain("animate-pulse");
  });

  it("keeps stale hits visible while a new burst refetches", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    let calls = 0;
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderBox({
      search: () => {
        calls += 1;
        if (calls === 1) return ISSUE_RESULTS;
        let gate!: () => void;
        const pending = new Promise<void>((resolve) => {
          gate = resolve;
        });
        release = gate;
        return pending.then(() => ISSUE_RESULTS);
      },
    });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER);
    await user.click(input);
    await user.type(input, "Par");
    await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 });
    await user.type(input, "ity");
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(screen.getByText(/Search results for/).className).toContain("animate-pulse");
    expect(screen.getByRole("option", { name: /Fix login/ })).toBeInTheDocument();
    expect(screen.queryByText("Searching")).not.toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.getByText(/Search results for/).className).not.toContain("animate-pulse"), {
      timeout: 5000,
    });
    expect(calls).toBe(2);
  });

  it("fires zero network with commands only while blank", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const { calls } = renderBox();
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    expect(await screen.findByRole("option", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(0);
  });

  it("fires zero network without a scope", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const { calls } = renderBox({ scope: null });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "Parity");
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(0);
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it("activates a server hit through the handler", async () => {
    const user = userEvent.setup();
    const onSelectSearchResult = vi.fn();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderBox({ search: () => ISSUE_RESULTS, onSelectSearchResult });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "Parity");
    await user.click(await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 }));
    expect(onSelectSearchResult).toHaveBeenCalledOnce();
    expect(onSelectSearchResult).toHaveBeenCalledWith(expect.objectContaining({ entity: "issue", title: "Fix login" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("clears the term from the no-hits row", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderBox({ search: () => ({ results: {} }) });
    const input = screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER) as HTMLInputElement;
    await user.click(input);
    await user.type(input, "zzzqqxnomatch");
    expect(await screen.findByText("No results found", undefined, { timeout: 5000 })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /No results found/ }));
    expect(input.value).toBe("");
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it("renders a dead backend as the no-hits row, never an error", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderBox({
      search: () => {
        throw new Error("backend unreachable");
      },
    });
    await user.click(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER));
    await user.type(screen.getByPlaceholderText(TOPBAR_SEARCH_PLACEHOLDER), "Parity");
    expect(await screen.findByText("No results found", undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByText(/error|failed|unreachable/i)).not.toBeInTheDocument();
  });
});
