// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles palette wiring (parity/specs/cycles.md §6, CYC-014): the create
// entry is registered only inside a project whose cycles switch is on
// and whose viewer is a member or admin, the `nc` sequence fires
// through the shell registry, and the command opens the cycles-owned
// create dialog for the project in context.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The create dialog rides the cycles chunk; this stand-in proves the
// command opened it without pulling the form's own reads.
vi.mock("../dialog/CycleDialog.js", () => ({
  CreateCycleDialog: (props: {
    open: boolean;
    workspaceSlug: string;
    projectId: string;
    currentProject: { id: string; name: string; identifier: string };
    onOpenChange: (next: boolean) => void;
  }) =>
    React.createElement(
      "div",
      {
        "data-testid": "cycle-create-dialog",
        "data-open": String(props.open),
        "data-project-id": props.projectId,
        "data-project-name": props.currentProject.name,
        "data-project-identifier": props.currentProject.identifier,
      },
      React.createElement("button", { type: "button", onClick: () => props.onOpenChange(false) }, "Close")
    ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findCommandBySequence,
  useCommandPalette,
} from "../../../shared/commands/index.js";
import { CommandPalette } from "../../../shared/shell/CommandPalette.js";
import { useGlobalShortcuts } from "../../../shared/shell/globalShortcuts.js";
import { CYCLES_PROJECT_ID, jsonResponse, makeProjectRow } from "../testing.js";
import { CYCLE_CREATE_COMMAND_ID } from "./cycleCommands.js";
import { CyclesPaletteEntries } from "./CyclesPaletteEntries.js";

const harness = vi.hoisted(
  (): {
    project: Record<string, unknown>;
    projectDelayMs: number;
    roles: unknown;
    rolesStatus: number;
  } => ({ project: {}, projectDelayMs: 0, roles: {}, rolesStatus: 200 })
);

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function renderEntries(): { unmount: () => void } {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CyclesPaletteEntries workspaceSlug="acme" projectId={CYCLES_PROJECT_ID} />
    </QueryClientProvider>
  );
}

function ShellProbe(): React.ReactElement {
  useGlobalShortcuts();
  return <div data-testid="shell-probe" />;
}

function renderShell(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ShellProbe />
      <CommandPalette scope={null} onSelectSearchResult={() => undefined} />
      <CyclesPaletteEntries workspaceSlug="acme" projectId={CYCLES_PROJECT_ID} />
    </QueryClientProvider>
  );
}

function pressSequence(sequence: string): void {
  for (const key of sequence) fireEvent.keyDown(window, { key });
}

beforeEach(() => {
  harness.project = makeProjectRow({ cycle_view: true });
  harness.projectDelayMs = 0;
  harness.roles = { [CYCLES_PROJECT_ID]: 15 };
  harness.rolesStatus = 200;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, 200, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, harness.rolesStatus, harness.roles);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
});

describe("CyclesPaletteEntries", () => {
  it("registers creation for a member in an enabled project with the nc sequence", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toMatchObject({
      title: "New cycle",
      section: "Create",
      sequence: "nc",
      hint: "Website relaunch",
    });
    expect(findCommandBySequence("nc")?.id).toBe(CYCLE_CREATE_COMMAND_ID);
  });

  it("registers creation for a project admin too", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 20 };
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommandBySequence("nc")?.id).toBe(CYCLE_CREATE_COMMAND_ID);
  });

  it("registers nothing for a project guest", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined());
    // The project read landed (command re-registered on the same render),
    // so the missing entry is gated, not still loading.
    await sleep(20);
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nc")).toBeUndefined();
  });

  it("fails creation closed when the standing read is missing or errors", async () => {
    harness.roles = {};
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined());
    await sleep(20);
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nc")).toBeUndefined();
  });

  it("registers nothing while the cycles switch is off", async () => {
    harness.project = makeProjectRow({ cycle_view: false });
    harness.roles = { [CYCLES_PROJECT_ID]: 20 };
    renderEntries();
    // No jump entry to wait on, so wait for the project read landing via a
    // late-load proof: flip the flag on in the harness is not possible
    // after mount, so use a short settle instead.
    await sleep(50);
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nc")).toBeUndefined();
  });

  it("registers nothing while the project loads, then the entry once it lands", async () => {
    harness.projectDelayMs = 50;
    renderEntries();
    await sleep(10);
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
  });

  it("unregisters the entry on unmount", async () => {
    const view = renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
    view.unmount();
    expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nc")).toBeUndefined();
  });

  it("opens the cycles-owned create dialog scoped to the project in context", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
    expect(screen.queryByTestId("cycle-create-dialog")).not.toBeInTheDocument();
    await findCommand(CYCLE_CREATE_COMMAND_ID)?.run();
    const dialog = await screen.findByTestId("cycle-create-dialog");
    expect(dialog).toHaveAttribute("data-open", "true");
    expect(dialog).toHaveAttribute("data-project-id", CYCLES_PROJECT_ID);
    expect(dialog).toHaveAttribute("data-project-name", "Website relaunch");
    expect(dialog).toHaveAttribute("data-project-identifier", "WEB");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByTestId("cycle-create-dialog")).not.toBeInTheDocument());
  });

  it("the nc sequence opens the create dialog from a project page", async () => {
    renderShell();
    await waitFor(() => expect(findCommandBySequence("nc")).toBeDefined());
    pressSequence("nc");
    expect(await screen.findByTestId("cycle-create-dialog")).toBeInTheDocument();
  });

  it("keeps the nc sequence registered after the palette was used", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(CYCLE_CREATE_COMMAND_ID)).toBeDefined());
    const before = findCommandBySequence("nc");
    useCommandPalette.getState().openPalette();
    useCommandPalette.getState().closePalette();
    expect(findCommandBySequence("nc")).toBe(before);
  });
});
