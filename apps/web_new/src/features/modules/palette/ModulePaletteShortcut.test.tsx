// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The `nm` creation sequence beside the shell palette (parity/specs/modules.md
// §10.2, §11.1, MOD-037/040). It opens the modules dialog from a project
// page, and it keeps working after a palette command was click-activated:
// the old app loses it until navigation (NEWFRONT-333), the shell here
// reads the live registry on every key, so a click cannot disarm it.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => routerHarness.navigate }));

vi.mock("../components/dialogs/ModuleSaveDialog.js", () => ({
  ModuleSaveDialog: () => React.createElement("div", { "data-testid": "module-save-dialog" }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommandBySequence,
  registerCommands,
  useCommandPalette,
} from "../../../shared/commands/index.js";
import { CommandPalette } from "../../../shared/shell/CommandPalette.js";
import { useGlobalShortcuts } from "../../../shared/shell/globalShortcuts.js";
import { jsonResponse, makeProjectRow, MODULES_PROJECT_ID } from "../testing.js";
import { ModulePaletteEntries } from "./ModulePaletteEntries.js";

function ShellProbe(): React.ReactElement {
  useGlobalShortcuts();
  return <div data-testid="shell-probe" />;
}

function renderShell(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ShellProbe />
      <CommandPalette scope={null} onSelectSearchResult={() => undefined} />
      <ModulePaletteEntries workspaceSlug="acme" projectId={MODULES_PROJECT_ID} />
    </QueryClientProvider>
  );
}

/** Type a two-key sequence into the page, as the driver's presses do. */
function pressSequence(sequence: string): void {
  for (const key of sequence) fireEvent.keyDown(window, { key });
}

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`)) {
      return jsonResponse(url, 200, makeProjectRow({ module_view: true }));
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, { [MODULES_PROJECT_ID]: 15 });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
      return jsonResponse(url, 200, []);
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

describe("the nm sequence", () => {
  it("opens the modules create dialog from a project page", async () => {
    renderShell();
    // The entries register after the project row and roles resolve.
    await waitFor(() => expect(findCommandBySequence("nm")).toBeDefined());
    pressSequence("nm");
    expect(await screen.findByTestId("module-save-dialog")).toBeInTheDocument();
  });

  it("keeps working after a palette command was click-activated", async () => {
    const run = vi.fn();
    registerCommands([{ id: "test.somewhere", title: "Do a nothing", section: "General", run }]);
    renderShell();
    await waitFor(() => expect(findCommandBySequence("nm")).toBeDefined());

    useCommandPalette.getState().openPalette();
    fireEvent.click(await screen.findByRole("option", { name: /Do a nothing/ }));
    expect(run).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(useCommandPalette.getState().open).toBe(false));

    pressSequence("nm");
    expect(await screen.findByTestId("module-save-dialog")).toBeInTheDocument();
  });
});
