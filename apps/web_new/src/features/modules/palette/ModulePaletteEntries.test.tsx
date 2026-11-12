// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules palette wiring (parity/specs/modules.md §10.2, MOD-037/040): the
// create entry is registered only inside a project whose modules switch is
// on and whose viewer is a member or admin, the jump entry whenever the
// switch is on, the jump page navigates to the picked module, and `nm`
// opens the modules-owned create dialog — including after a palette command
// was clicked (NEWFRONT-333).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => routerHarness.navigate }));

// The create dialog rides the modules chunk; this stand-in proves the
// command opened it without pulling the form's own project/member reads.
vi.mock("../components/dialogs/ModuleSaveDialog.js", () => ({
  ModuleSaveDialog: (props: { open: boolean; mode: string; onOpenChange: (next: boolean) => void }) =>
    React.createElement(
      "div",
      { "data-testid": "module-save-dialog", "data-open": String(props.open), "data-mode": props.mode },
      React.createElement("button", { type: "button", onClick: () => props.onOpenChange(false) }, "Close")
    ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findCommandBySequence,
  findPalettePage,
  useCommandPalette,
  type PalettePage,
} from "../../../shared/commands/index.js";
import { jsonResponse, makeModuleRow, makeProjectRow, MODULES_PROJECT_ID } from "../testing.js";
import { MODULE_CREATE_COMMAND_ID, MODULE_JUMP_COMMAND_ID, MODULE_JUMP_PAGE_ID } from "./moduleCommands.js";
import { ModulePaletteEntries } from "./ModulePaletteEntries.js";

const MODULE_ID = "323e4567-e89b-12d3-a456-426614174011";

const harness = vi.hoisted(
  (): {
    project: Record<string, unknown>;
    projectDelayMs: number;
    roles: unknown;
    rolesStatus: number;
    modules: unknown[];
    modulesStatus: number;
  } => ({ project: {}, projectDelayMs: 0, roles: {}, rolesStatus: 200, modules: [], modulesStatus: 200 })
);

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function renderEntries(): { unmount: () => void } {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulePaletteEntries workspaceSlug="acme" projectId={MODULES_PROJECT_ID} />
    </QueryClientProvider>
  );
}

function PageHarness({ page }: { page: PalettePage }): React.ReactElement {
  const state = page.usePageState("");
  return (
    <div>
      {state.options.map((option) => (
        <button
          key={option.id}
          type="button"
          aria-label={option.title}
          onClick={() => {
            void option.run();
          }}
        >
          {option.title}
        </button>
      ))}
    </div>
  );
}

beforeEach(() => {
  harness.project = makeProjectRow({ module_view: true });
  harness.projectDelayMs = 0;
  harness.roles = { [MODULES_PROJECT_ID]: 15 };
  harness.rolesStatus = 200;
  harness.modules = [];
  harness.modulesStatus = 200;
  routerHarness.navigate.mockReset();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, 200, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, harness.rolesStatus, harness.roles);
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
      return jsonResponse(url, harness.modulesStatus, harness.modules);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
});

describe("ModulePaletteEntries", () => {
  it("registers creation and jump for a member in an enabled project", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toMatchObject({
      title: "New module",
      section: "Create",
      sequence: "nm",
      hint: "Website relaunch",
    });
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toMatchObject({
      title: "Open a module",
      section: "Navigate",
      page: MODULE_JUMP_PAGE_ID,
      hint: "Website relaunch",
    });
    expect(findCommandBySequence("nm")?.id).toBe(MODULE_CREATE_COMMAND_ID);
    expect(findPalettePage(MODULE_JUMP_PAGE_ID)).toBeDefined();
  });

  it("registers creation for a project admin too", async () => {
    harness.roles = { [MODULES_PROJECT_ID]: 20 };
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined();
  });

  it("registers only the jump entry for a guest", async () => {
    harness.roles = { [MODULES_PROJECT_ID]: 5 };
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nm")).toBeUndefined();
  });

  it("fails creation closed when the standing read is missing or errors", async () => {
    harness.roles = {};
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();

    clearCommands();
    harness.rolesStatus = 500;
    harness.roles = { detail: "boom" };
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();
  });

  it("registers neither entry while the modules switch is off", async () => {
    harness.project = makeProjectRow({ module_view: false });
    harness.roles = { [MODULES_PROJECT_ID]: 20 };
    renderEntries();
    await waitFor(() => expect(findPalettePage(MODULE_JUMP_PAGE_ID)).toBeDefined());
    // The page registers on the render after the project row lands, so
    // absent commands here are gated rather than still loading.
    await sleep(20);
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nm")).toBeUndefined();
  });

  it("registers nothing while the project loads, then both once it lands", async () => {
    harness.projectDelayMs = 50;
    renderEntries();
    await sleep(10);
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeUndefined();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined();
  });

  it("unregisters both entries and the page on unmount", async () => {
    const view = renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    view.unmount();
    expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeUndefined();
    expect(findPalettePage(MODULE_JUMP_PAGE_ID)).toBeUndefined();
  });

  it("opens the modules-owned create dialog when the command fires", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    expect(screen.queryByTestId("module-save-dialog")).not.toBeInTheDocument();
    await findCommand(MODULE_CREATE_COMMAND_ID)?.run();
    const dialog = await screen.findByTestId("module-save-dialog");
    expect(dialog).toHaveAttribute("data-open", "true");
    expect(dialog).toHaveAttribute("data-mode", "create");
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByTestId("module-save-dialog")).not.toBeInTheDocument());
  });

  it("reopens a fresh dialog session after a dismissed one", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    await findCommand(MODULE_CREATE_COMMAND_ID)?.run();
    const first = await screen.findByTestId("module-save-dialog");
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByTestId("module-save-dialog")).not.toBeInTheDocument());
    await findCommand(MODULE_CREATE_COMMAND_ID)?.run();
    const second = await screen.findByTestId("module-save-dialog");
    expect(second).not.toBe(first);
  });

  it("navigates to the picked module's detail from the jump page", async () => {
    harness.modules = [makeModuleRow({ id: MODULE_ID, name: "Triage" })];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <ModulePaletteEntries workspaceSlug="acme" projectId={MODULES_PROJECT_ID} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(MODULE_JUMP_COMMAND_ID)).toBeDefined());
    const page = findPalettePage(MODULE_JUMP_PAGE_ID);
    if (page === undefined) throw new Error("jump page missing");
    render(
      <QueryClientProvider client={queryClient}>
        <PageHarness page={page} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument());
    useCommandPalette.getState().openPalette();
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULE_ID}`,
    });
    expect(useCommandPalette.getState().open).toBe(false);
  });

  it("keeps the nm sequence registered after the palette was used", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(MODULE_CREATE_COMMAND_ID)).toBeDefined());
    // Using a palette command is a store interaction, not a registry write;
    // the modules entries stay exactly as they were (NEWFRONT-333).
    const before = findCommandBySequence("nm");
    useCommandPalette.getState().openPalette();
    useCommandPalette.getState().closePalette();
    expect(findCommandBySequence("nm")).toBe(before);
  });
});
