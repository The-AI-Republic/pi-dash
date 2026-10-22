// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notifyError = vi.fn();
vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: (title: string, description?: string) => notifyError(title, description),
}));

import { InstallSection } from "./InstallSection.js";
import { COPY_COMMAND_LABEL, COPIED_LABEL, INSTALL_BLOCKS, INSTALL_HEADING, INSTALL_PREREQUISITE } from "./install.js";

afterEach(() => {
  vi.useRealTimers();
  notifyError.mockReset();
});

function installCard(index: number): HTMLElement {
  const command = INSTALL_BLOCKS[index]?.command ?? "";
  return screen.getByText((text) => text.replace(/\s+/g, " ") === command.replace(/\s+/g, " "))
    .parentElement as HTMLElement;
}

describe("InstallSection", () => {
  it("renders the heading, three cards and the prerequisite note", () => {
    render(<InstallSection writeClipboard={async () => undefined} />);
    expect(screen.getByRole("heading", { name: INSTALL_HEADING })).toBeInTheDocument();
    expect(screen.getByText(INSTALL_PREREQUISITE)).toBeInTheDocument();
    for (const block of INSTALL_BLOCKS) {
      expect(screen.getByText(block.label)).toBeInTheDocument();
      expect(
        screen.getByText((text) => text.replace(/\s+/g, " ") === block.command.replace(/\s+/g, " "))
      ).toBeInTheDocument();
    }
    expect(screen.getByRole("link", { name: /Download MSI/ })).toHaveAttribute("href", INSTALL_BLOCKS[2]?.download);
  });

  it("writes the displayed command and confirms transiently", async () => {
    const write = vi.fn(async () => undefined);
    render(<InstallSection writeClipboard={write} confirmMs={50} />);
    const card = installCard(0);
    const button = card.querySelector("button");
    if (button === null) throw new Error("missing copy button");
    expect(button).toHaveTextContent(COPY_COMMAND_LABEL);
    fireEvent.click(button);
    await waitFor(() => expect(write).toHaveBeenCalledWith(INSTALL_BLOCKS[0]?.command));
    await waitFor(() => expect(button).toHaveTextContent(COPIED_LABEL));
    await waitFor(() => expect(button).toHaveTextContent(COPY_COMMAND_LABEL), { timeout: 2_000 });
  });

  it("restarts the confirmation window on a repeated copy", async () => {
    const write = vi.fn(async () => undefined);
    render(<InstallSection writeClipboard={write} confirmMs={50} />);
    const button = installCard(1).querySelector("button");
    if (button === null) throw new Error("missing copy button");
    fireEvent.click(button);
    await waitFor(() => expect(button).toHaveTextContent(COPIED_LABEL));
    fireEvent.click(button);
    await waitFor(() => expect(write).toHaveBeenCalledTimes(2));
    expect(button).toHaveTextContent(COPIED_LABEL);
  });

  it("toasts instead of confirming when the clipboard refuses", async () => {
    const write = vi.fn(async () => {
      throw new Error("denied");
    });
    render(<InstallSection writeClipboard={write} />);
    const card = installCard(0);
    const button = card.querySelector("button");
    if (button === null) throw new Error("missing copy button");
    fireEvent.click(button);
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(button).toHaveTextContent(COPY_COMMAND_LABEL);
  });

  it("uses the navigator clipboard by default", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<InstallSection />);
    const card = installCard(1);
    const button = card.querySelector("button");
    if (button === null) throw new Error("missing copy button");
    fireEvent.click(button);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(INSTALL_BLOCKS[1]?.command));
  });
});
