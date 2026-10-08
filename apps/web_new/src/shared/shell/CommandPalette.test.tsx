// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearCommands, registerCommands, useCommandPalette } from "../commands/index.js";
import { CommandPalette } from "./CommandPalette.js";

describe("CommandPalette", () => {
  afterEach(() => {
    clearCommands();
    useCommandPalette.getState().closePalette();
  });

  it("filters registered commands and runs the chosen one", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    registerCommands([
      { id: "shell.sign-out", title: "Sign out", section: "Account", run },
      { id: "shell.go-home", title: "Go to workspace home", section: "Navigate", run: () => undefined },
    ]);
    render(<CommandPalette />);
    useCommandPalette.getState().openPalette();
    expect(await screen.findByRole("listbox", { name: "Commands" })).toBeInTheDocument();
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
    render(<CommandPalette />);
    useCommandPalette.getState().openPalette();
    await screen.findByRole("listbox", { name: "Commands" });
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledOnce();
  });

  it("names the empty result", async () => {
    const user = userEvent.setup();
    registerCommands([{ id: "shell.sign-out", title: "Sign out", run: () => undefined }]);
    render(<CommandPalette />);
    useCommandPalette.getState().openPalette();
    await screen.findByRole("listbox", { name: "Commands" });
    await user.type(screen.getByLabelText("Filter commands"), "nothing-matches-this");
    expect(screen.getByText("No matching commands.")).toBeInTheDocument();
  });
});
