// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CLI_INSTALL_TABS } from "../onboarding.js";
import { CLI_COPY_FAILED_TITLE, CliInstallStep } from "./CliInstallStep.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.mocked(notifyError).mockClear();
});

describe("CliInstallStep", () => {
  it("shows the Unix guidance by default with both continue actions", async () => {
    const user = userEvent.setup();
    const onAdvance = vi.fn();
    render(<CliInstallStep onAdvance={onAdvance} />);

    expect(screen.getByRole("heading", { name: "Connect your AI agents with Pi Dash" })).toBeInTheDocument();
    expect(screen.getByText(/hand tasks to those agents/)).toBeInTheDocument();
    expect(screen.getByText(/machine where the coding agent will run the work/)).toBeInTheDocument();
    expect(screen.getByText(CLI_INSTALL_TABS.unix.commands[0]?.command ?? "")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "macOS / Linux" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(onAdvance).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Skip for now" }));
    expect(onAdvance).toHaveBeenCalledTimes(2);
  });

  it("preselects Windows for Windows browsers", () => {
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    render(<CliInstallStep onAdvance={() => undefined} />);
    expect(screen.getByRole("button", { name: "Windows" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(CLI_INSTALL_TABS.windows.commands[0]?.command ?? "")).toBeInTheDocument();
    expect(screen.getByText(CLI_INSTALL_TABS.windows.commands[1]?.command ?? "")).toBeInTheDocument();
  });

  it("switches tabs on demand", async () => {
    const user = userEvent.setup();
    render(<CliInstallStep onAdvance={() => undefined} />);
    await user.click(screen.getByRole("button", { name: "Windows" }));
    expect(screen.getByRole("button", { name: "Windows" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "macOS / Linux" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "macOS / Linux" }));
    expect(screen.getByRole("button", { name: "macOS / Linux" })).toHaveAttribute("aria-pressed", "true");
  });

  it("copies a command and confirms it", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { userAgent: "unix", clipboard: { writeText } });
    render(<CliInstallStep onAdvance={() => undefined} />);

    await user.click(screen.getByRole("button", { name: "Copy Terminal command" }));
    expect(writeText).toHaveBeenCalledWith(CLI_INSTALL_TABS.unix.commands[0]?.command);
    expect(screen.getByRole("button", { name: "Copied Terminal command" })).toHaveTextContent("Copied");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("explains manual copying when copying is unavailable", async () => {
    const user = userEvent.setup();
    vi.stubGlobal("navigator", { userAgent: "unix" });
    (document as unknown as Record<string, unknown>)["execCommand"] = vi.fn().mockReturnValue(false);
    render(<CliInstallStep onAdvance={() => undefined} />);

    await user.click(screen.getByRole("button", { name: "Copy Terminal command" }));
    expect(notifyError).toHaveBeenCalledWith(CLI_COPY_FAILED_TITLE, expect.any(String));
    expect(screen.queryByRole("button", { name: "Copied Terminal command" })).toBeNull();
  });
});
