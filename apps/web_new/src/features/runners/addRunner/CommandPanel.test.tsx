// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command panel (RUN-008): shell switching, transient copy confirmation,
// clipboard failure toast and the origin note.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const notifyError = vi.fn();
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: (title: string, message?: string) => notifyError(title, message) };
});

import { AddRunnerCommandPanel, COPY_CONFIRM_MS, COPY_FAILED_MESSAGE, ORIGIN_NOTE } from "./CommandPanel.js";

const FIELDS = {
  cloudUrl: "https://cloud.test",
  workspaceSlug: "acme",
  projectIdentifier: "ACME",
  podName: "",
  runnerName: "laptop_1",
  workingDir: "/tmp/my dir",
  agent: "codex",
  model: "gpt-5.6-sol",
  reasoningEffort: "high",
};

const writeText = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  notifyError.mockReset();
  writeText.mockReset();
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
});

describe("AddRunnerCommandPanel", () => {
  it("renders the POSIX command by default and switches shells", async () => {
    render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    const pre = screen.getByTestId("add-runner-command-text");
    expect(pre.textContent).toContain("pidash runner add \\");
    expect(pre.textContent).toContain("--working-dir '/tmp/my dir'");
    expect(screen.getByRole("button", { name: "macOS/Linux" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "Command Prompt" }));
    expect(screen.getByRole("button", { name: "Command Prompt" })).toHaveAttribute("aria-pressed", "true");
    expect(pre.textContent).not.toContain("\n");
    expect(pre.textContent).toContain('--working-dir "/tmp/my dir"');
  });

  it("copies with a transient confirmation", async () => {
    render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    const command = screen.getByTestId("add-runner-command-text").textContent ?? "";
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Copied!" })).toBeInTheDocument());
    expect(writeText).toHaveBeenCalledWith(command);
    await waitFor(() => expect(screen.getByRole("button", { name: "Copy command" })).toBeInTheDocument(), {
      timeout: COPY_CONFIRM_MS + 2_000,
    });
  });

  it("restarts the confirmation window when copied again", async () => {
    render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Copied!" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Copied!" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("button", { name: "Copied!" })).toBeInTheDocument();
  });

  it("toasts when the clipboard refuses", async () => {
    writeText.mockRejectedValue(new Error("blocked"));
    render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Error!", COPY_FAILED_MESSAGE));
    expect(screen.getByRole("button", { name: "Copy command" })).toBeInTheDocument();
  });

  it("shows the origin note only on the fallback path", () => {
    const { unmount } = render(<AddRunnerCommandPanel fields={FIELDS} originFallback />);
    expect(screen.getByText(ORIGIN_NOTE)).toBeInTheDocument();
    unmount();
    render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    expect(screen.queryByText(ORIGIN_NOTE)).toBeNull();
  });

  it("does not write state after unmount when a confirm timer is pending", async () => {
    const clear = vi.spyOn(globalThis, "clearTimeout");
    const { unmount } = render(<AddRunnerCommandPanel fields={FIELDS} originFallback={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Copied!" })).toBeInTheDocument());
    unmount();
    // The pending confirmation timer is cancelled on unmount.
    expect(clear).toHaveBeenCalled();
    clear.mockRestore();
  });
});
