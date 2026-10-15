// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
import { CommandPalette, PALETTE_ROOT_PLACEHOLDER } from "./CommandPalette.js";

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

function jsonResponse(url: string, value: unknown): TransportResponse {
  return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(value) };
}

function renderPalette(
  options: {
    scope?: PaletteSearchScope | null;
    onSelectSearchResult?: (hit: SearchResultHit) => void;
    search?: (url: string) => unknown | Promise<unknown>;
  } = {}
): { calls: string[]; queryClient: QueryClient } {
  const calls: string[] = [];
  createAppClient("https://api.test", async (url) => {
    calls.push(url);
    const body = options.search ? await options.search(url) : { results: {} };
    return jsonResponse(url, body);
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CommandPalette
        scope={options.scope === undefined ? SCOPE : options.scope}
        onSelectSearchResult={options.onSelectSearchResult ?? (() => undefined)}
      />
    </QueryClientProvider>
  );
  useCommandPalette.getState().openPalette();
  return { calls, queryClient };
}

function searchParams(url: string): Record<string, string> {
  return Object.fromEntries(new URL(url).searchParams.entries());
}

function selectedOption(): HTMLElement | null {
  return document.querySelector('[role="option"][aria-selected="true"]');
}

afterEach(() => {
  // No resetAppClient: the dialog's close transition can re-render after
  // this hook, and every render configures a fresh client up front.
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
});

describe("CommandPalette commands", () => {
  it("filters registered commands and runs the chosen one", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([
      { id: "shell.sign-out", title: "Sign out", section: "Account", run },
      { id: "shell.go-home", title: "Go to workspace home", section: "Navigation", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    expect(await screen.findByRole("listbox", { name: "Commands" })).toBeInTheDocument();
    expect(screen.getByText("Account")).toBeInTheDocument();
    expect(screen.getByText("Navigation")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Filter commands"), "sign");
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Go to workspace home" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Sign out" }));
    expect(run).toHaveBeenCalledOnce();
    expect(useCommandPalette.getState().open).toBe(false);
  });

  it("runs the highlighted command on Enter", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([{ id: "issues.refresh-list", title: "Refresh issue list", run }]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
  });

  it("names the empty result", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.type(screen.getByLabelText("Filter commands"), "nothing-matches-this");
    expect(screen.getByText("No matching commands.")).toBeInTheDocument();
  });

  it("moves the highlight on hover", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "a", title: "Alpha", run: () => undefined },
      { id: "b", title: "Beta", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    expect(selectedOption()).toHaveTextContent("Alpha");
    await user.hover(screen.getByRole("option", { name: "Beta" }));
    expect(selectedOption()).toHaveTextContent("Beta");
  });
});

describe("CommandPalette open and reset (SHELL-080/082)", () => {
  function registerPicker(): void {
    registerCommands([
      { id: "shell.go-home", title: "Go to workspace home", run: () => undefined },
      { id: "project.open", title: "Open a project", page: "test.project-picker", run: () => undefined },
    ]);
    registerPalettePages([
      {
        id: "test.project-picker",
        placeholder: "Search projects",
        usePageState: (): PalettePageState => ({ status: "ready", options: [], emptyText: "No projects found" }),
      },
    ]);
  }

  it("opens at the root prompt and resets on close", async () => {
    const user = userEvent.setup();
    registerPicker();
    renderPalette({ scope: null });
    const input = (await screen.findByLabelText("Filter commands")) as HTMLInputElement;
    expect(input.placeholder).toBe(PALETTE_ROOT_PLACEHOLDER);

    await user.type(input, "hello");
    expect(input.value).toBe("hello");
    await user.keyboard("{Escape}");
    expect(input.value).toBe("");
    expect(screen.getByRole("listbox", { name: "Commands" })).toBeInTheDocument();

    await user.click(screen.getByRole("option", { name: "Open a project" }));
    const pageInput = (await screen.findByLabelText("Filter options")) as HTMLInputElement;
    expect(pageInput.placeholder).toBe("Search projects");
    expect(pageInput.value).toBe("");

    const backdrop = document.querySelector("div.bg-black\\/40");
    expect(backdrop).not.toBeNull();
    await user.click(backdrop as HTMLElement);
    await waitFor(() => expect(useCommandPalette.getState().open).toBe(false));

    useCommandPalette.getState().openPalette();
    const reopened = (await screen.findByLabelText("Filter commands")) as HTMLInputElement;
    expect(reopened.value).toBe("");
    expect(reopened.placeholder).toBe(PALETTE_ROOT_PLACEHOLDER);
    expect(screen.getByRole("option", { name: "Go to workspace home" })).toBeInTheDocument();
  });

  it("renders the root list for an unknown page id", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "shell.go-home", title: "Go to workspace home", run: () => undefined },
      { id: "stale", title: "Stale entry", page: "missing.page", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Stale entry" }));
    expect(useCommandPalette.getState().page).toBe("missing.page");
    expect((screen.getByLabelText("Filter commands") as HTMLInputElement).placeholder).toBe(PALETTE_ROOT_PLACEHOLDER);
    expect(screen.getByRole("option", { name: "Go to workspace home" })).toBeInTheDocument();
  });
});

describe("CommandPalette keyboard flow (SHELL-083)", () => {
  it("moves, activates, clears-then-closes", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([
      { id: "a", title: "Alpha", run: () => undefined },
      { id: "b", title: "Beta", run },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });

    expect(selectedOption()).toHaveTextContent("Alpha");
    await user.keyboard("{ArrowDown}");
    expect(selectedOption()).toHaveTextContent("Beta");
    await user.keyboard("{ArrowUp}");
    expect(selectedOption()).toHaveTextContent("Alpha");
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
    expect(useCommandPalette.getState().open).toBe(false);

    useCommandPalette.getState().openPalette();
    const input = (await screen.findByLabelText("Filter commands")) as HTMLInputElement;
    await user.type(input, "abc");
    expect(input.value).toBe("abc");
    await user.keyboard("{Escape}");
    expect(input.value).toBe("");
    expect(screen.getByRole("listbox", { name: "Commands" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(useCommandPalette.getState().open).toBe(false));
  });

  it("activates a tabbed-to row exactly once on Enter", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([
      { id: "a", title: "Alpha", run },
      { id: "b", title: "Beta", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.tab();
    expect(screen.getByRole("option", { name: "Alpha" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
  });

  it("steps back from a sub-page on Backspace with an empty query", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "shell.go-home", title: "Go to workspace home", run: () => undefined },
      { id: "project.open", title: "Open a project", page: "test.project-picker", run: () => undefined },
    ]);
    registerPalettePages([
      {
        id: "test.project-picker",
        placeholder: "Search projects",
        usePageState: (): PalettePageState => ({ status: "ready", options: [], emptyText: "No projects found" }),
      },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.keyboard("{Backspace}");
    expect(useCommandPalette.getState().open).toBe(true);
    expect((screen.getByLabelText("Filter commands") as HTMLInputElement).placeholder).toBe(PALETTE_ROOT_PLACEHOLDER);

    await user.click(screen.getByRole("option", { name: "Open a project" }));
    await screen.findByLabelText("Filter options");
    await user.keyboard("{Backspace}");
    expect((await screen.findByLabelText("Filter commands")).getAttribute("placeholder")).toBe(
      PALETTE_ROOT_PLACEHOLDER
    );
  });
});

describe("CommandPalette sub-pages (SHELL-093)", () => {
  function registerPage(usePageState: (query: string) => PalettePageState): void {
    registerCommands([
      { id: "project.open", title: "Open a project", page: "test.project-picker", run: () => undefined },
    ]);
    registerPalettePages([{ id: "test.project-picker", placeholder: "Search projects", usePageState }]);
  }

  it("spins while the picker loads", async () => {
    const user = userEvent.setup();
    registerPage(() => ({ status: "loading", options: [], emptyText: "No projects found" }));
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Open a project" }));
    expect(await screen.findByText("Loading options")).toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("shows a plain empty line with no data", async () => {
    const user = userEvent.setup();
    registerPage(() => ({ status: "ready", options: [], emptyText: "No projects found" }));
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Open a project" }));
    expect(await screen.findByText("No projects found")).toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("runs options without closing and activates on Enter", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerPage((query) => ({
      status: "ready",
      options:
        query === ""
          ? [
              { id: "p1", title: "Atlas", hint: "ATL", run: () => undefined },
              { id: "p2", title: "Beacon", hint: "BCN", run },
            ]
          : [],
      emptyText: "No projects found",
    }));
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Open a project" }));
    await screen.findByRole("listbox", { name: "Options" });
    expect(screen.getByRole("option", { name: /Beacon/ })).toHaveAttribute("aria-selected", "false");
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
    expect(useCommandPalette.getState().open).toBe(true);
    expect(screen.getByRole("listbox", { name: "Options" })).toBeInTheDocument();
    await user.hover(screen.getByRole("option", { name: /Atlas/ }));
    expect(screen.getByRole("option", { name: /Atlas/ })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("option", { name: /Atlas/ }));
    expect(run).toHaveBeenCalledOnce();
    expect(useCommandPalette.getState().open).toBe(true);
  });
});

describe("CommandPalette blank query (SHELL-085)", () => {
  it("lists grouped commands with zero network and no results section", async () => {
    registerCommands([
      { id: "shell.sign-out", title: "Sign out", section: "Account", run: () => undefined },
      { id: "shell.go-home", title: "Go to workspace home", section: "Navigation", run: () => undefined },
    ]);
    const { calls } = renderPalette();
    await screen.findByRole("listbox", { name: "Commands" });
    expect(screen.getByText("Account")).toBeInTheDocument();
    expect(screen.getByText("Navigation")).toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
    expect(screen.queryByText(/recent|history/i)).not.toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(0);
  });
});

describe("CommandPalette server search (SHELL-084)", () => {
  it("coalesces a burst into one project-scoped request with grouped hits", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const { calls } = renderPalette({ search: () => ISSUE_RESULTS });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.type(screen.getByLabelText("Filter commands"), "Parity");
    expect(await screen.findByText(/Search results for/, undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByText("Issues")).toBeInTheDocument();
    expect(screen.getByLabelText("Server results")).not.toHaveAttribute("aria-busy");
    expect(screen.queryByText(/recent|history/i)).not.toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(1);
    expect(searchParams(calls[0]!)).toEqual({ search: "Parity", project_id: PROJECT_ID, workspace_search: "false" });
  });

  it("pulses the heading and spins while the search flies", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({
      search: () => gate.then(() => ISSUE_RESULTS),
    });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    const heading = await screen.findByText(/Search results for/, undefined, { timeout: 5000 });
    expect(heading.className).toContain("animate-pulse");
    expect(screen.getByLabelText("Server results")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Searching")).toBeInTheDocument();
    release();
    expect(await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 })).toBeInTheDocument();
    expect(heading.className).not.toContain("animate-pulse");
  });

  it("widens to the workspace through the footer toggle", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    let calls = 0;
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const rendered = renderPalette({
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
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 });
    const toggle = screen.getByRole("switch", { name: "Workspace level" });
    expect(toggle).toBeEnabled();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(rendered.calls).toHaveLength(2), { timeout: 5000 });
    // Stale hits stay visible while the widened search flies.
    expect(screen.getByText(/Search results for/).className).toContain("animate-pulse");
    expect(screen.getByRole("option", { name: /Fix login/ })).toBeInTheDocument();
    expect(screen.queryByText("Searching")).not.toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.getByText(/Search results for/).className).not.toContain("animate-pulse"), {
      timeout: 5000,
    });
    expect(searchParams(rendered.calls[1]!)).toMatchObject({ search: "Parity", workspace_search: "true" });
    expect("project_id" in searchParams(rendered.calls[1]!)).toBe(true);
  });

  it("disables the scope toggle outside a project", async () => {
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ scope: { workspaceSlug: "acme", projectId: null }, search: () => ISSUE_RESULTS });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 });
    expect(screen.getByRole("switch", { name: "Workspace level" })).toBeDisabled();
  });

  it("renders commands only without a scope", async () => {
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    const { calls } = renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(calls).toHaveLength(0);
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
    expect(screen.getByText("No matching commands.")).toBeInTheDocument();
  });

  it("activates a server hit through the handler", async () => {
    const user = userEvent.setup();
    const onSelectSearchResult = vi.fn();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ search: () => ISSUE_RESULTS, onSelectSearchResult });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    const hit = await screen.findByRole("option", { name: /Fix login/ }, { timeout: 5000 });
    await user.hover(hit);
    expect(hit).toHaveAttribute("aria-selected", "true");
    await user.click(hit);
    expect(onSelectSearchResult).toHaveBeenCalledOnce();
    expect(onSelectSearchResult).toHaveBeenCalledWith(
      expect.objectContaining({ entity: "issue", title: "Fix login", path: "ATL-12" })
    );
    expect(useCommandPalette.getState().open).toBe(false);
  });
});

describe("CommandPalette no-hits row (SHELL-093)", () => {
  it("clears the query when the no-hits row activates", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ search: () => ({ results: {} }) });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("zzzqqxnomatch");
    expect(await screen.findByText("No results found", undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByText("Clear search")).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /No results found/ }));
    expect((screen.getByLabelText("Filter commands") as HTMLInputElement).value).toBe("");
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it("activates the no-hits row on Enter", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ search: () => ({ results: {} }) });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("zzzqqxnomatch");
    await screen.findByText("No results found", undefined, { timeout: 5000 });
    await user.keyboard("{Enter}");
    expect((screen.getByLabelText("Filter commands") as HTMLInputElement).value).toBe("");
  });

  it("lists the no-hits row after matching commands", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "shell.sign-out", title: "Sign out", run: () => undefined },
      { id: "shell.sign-in", title: "Sign in", run: () => undefined },
    ]);
    renderPalette({ search: () => ({ results: {} }) });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("sign");
    const clear = await screen.findByRole("option", { name: /No results found/ }, { timeout: 5000 });
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
    expect(clear).toHaveAttribute("aria-selected", "false");
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{ArrowDown}");
    expect(clear).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Enter}");
    expect((screen.getByLabelText("Filter commands") as HTMLInputElement).value).toBe("");
  });

  it("renders a dead backend as the no-hits row, never an error", async () => {
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({
      search: () => {
        throw new Error("backend unreachable");
      },
    });
    await screen.findByRole("listbox", { name: "Commands" });
    useCommandPalette.getState().setQuery("Parity");
    expect(await screen.findByText("No results found", undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByText(/error|failed|unreachable/i)).not.toBeInTheDocument();
  });
});

describe("CommandPalette context chip (SHELL-090/091)", () => {
  const CONTEXT = { kind: "work-item" as const, indicator: "Work item", subject: "ACME-12" };

  it("names the subject and groups its commands", async () => {
    registerCommands([
      { id: "ctx.state", title: "Change state", section: "Work item actions", context: CONTEXT, run: () => undefined },
      { id: "shell.sign-out", title: "Sign out", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    expect(screen.getByText("Work item")).toBeInTheDocument();
    expect(screen.getByText("ACME-12")).toBeInTheDocument();
    expect(screen.getByText("Work item actions")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Change state" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
  });

  it("hides the chip and group on clear and restores them on reopen", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "ctx.state", title: "Change state", section: "Work item actions", context: CONTEXT, run: () => undefined },
      { id: "shell.sign-out", title: "Sign out", run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("button", { name: "Clear context (Backspace)" }));
    expect(screen.queryByText("ACME-12")).not.toBeInTheDocument();
    expect(screen.queryByText("Work item actions")).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Change state" })).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Sign out" })).toBeInTheDocument();
    useCommandPalette.getState().closePalette();
    useCommandPalette.getState().openPalette();
    expect(await screen.findByText("ACME-12")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Change state" })).toBeInTheDocument();
  });

  it("dismisses the context on Backspace with an empty query", async () => {
    const user = userEvent.setup();
    registerCommands([
      { id: "ctx.state", title: "Change state", section: "Work item actions", context: CONTEXT, run: () => undefined },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByLabelText("Filter commands"));
    await user.keyboard("{Backspace}");
    expect(screen.queryByText("ACME-12")).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Change state" })).not.toBeInTheDocument();
  });

  it("renders no chip without contextual commands", async () => {
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    expect(screen.queryByRole("button", { name: "Clear context (Backspace)" })).not.toBeInTheDocument();
  });

  it("marks selected picker options with a check", async () => {
    const user = userEvent.setup();
    registerCommands([
      {
        id: "ctx.state",
        title: "Change state",
        section: "Work item actions",
        context: CONTEXT,
        page: "ctx.state-page",
        run: () => undefined,
      },
    ]);
    registerPalettePages([
      {
        id: "ctx.state-page",
        placeholder: "Change state",
        usePageState: (): PalettePageState => ({
          status: "ready",
          options: [
            { id: "a", title: "Triage", selected: true, run: () => undefined },
            { id: "b", title: "Done", run: () => undefined },
          ],
          emptyText: "No states found",
        }),
      },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Change state" }));
    const options = await screen.findByRole("listbox", { name: "Options" });
    expect(options).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Triage" })).toHaveTextContent("✓");
    expect(screen.getByRole("option", { name: "Done" })).not.toHaveTextContent("✓");
  });

  it("steps back to the root list when the chip clears from a sub-page", async () => {
    const user = userEvent.setup();
    registerCommands([
      {
        id: "ctx.state",
        title: "Change state",
        section: "Work item actions",
        context: CONTEXT,
        page: "ctx.state-page",
        run: () => undefined,
      },
    ]);
    registerPalettePages([
      {
        id: "ctx.state-page",
        placeholder: "Change state",
        usePageState: (): PalettePageState => ({
          status: "ready",
          options: [{ id: "a", title: "Triage", run: () => undefined }],
          emptyText: "No states found",
        }),
      },
    ]);
    renderPalette({ scope: null });
    await screen.findByRole("listbox", { name: "Commands" });
    await user.click(screen.getByRole("option", { name: "Change state" }));
    await screen.findByRole("listbox", { name: "Options" });
    await user.click(screen.getByRole("button", { name: "Clear context (Backspace)" }));
    await screen.findByRole("listbox", { name: "Commands" });
    expect(screen.queryByRole("option", { name: "Change state" })).not.toBeInTheDocument();
  });
});
