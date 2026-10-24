// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { PeekChrome } from "./PeekChrome.js";

function setup(props?: Partial<React.ComponentProps<typeof PeekChrome>>) {
  const onClose = vi.fn();
  const onModeChange = vi.fn();
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  const view = render(
    <PeekChrome mode="side" onModeChange={onModeChange} title="Fix login" onClose={onClose} {...props} />
  );
  return { onClose, onModeChange, ...view };
}

describe("PeekChrome", () => {
  it("renders the panel shell with the mode toggle and body", async () => {
    const user = userEvent.setup();
    setup({ eyebrow: "WEB-42", actions: <button type="button">Custom</button>, children: <p>Peek body</p> });
    expect(screen.getByTestId("issue-preview")).toHaveAttribute("data-peek-layout", "side");
    expect(screen.getByText("WEB-42")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(screen.getByRole("button", { name: "Custom" })).toBeInTheDocument();
    expect(screen.getByText("Peek body")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    expect(await screen.findByRole("menuitem", { name: "Side peek" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
  });

  it("switches the panel presentation through the toggle", async () => {
    const user = userEvent.setup();
    const { onModeChange } = setup();
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    expect(onModeChange).toHaveBeenCalledWith("modal");
  });

  it("closes on outside presses and honors suppression", () => {
    const first = setup();
    fireEvent.pointerDown(document.body);
    expect(first.onClose).toHaveBeenCalledTimes(1);
    first.unmount();
    const embedded = setup({ embedded: true });
    fireEvent.pointerDown(document.body);
    expect(embedded.onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(embedded.onClose).not.toHaveBeenCalled();
    embedded.unmount();
  });

  it("stands Escape down while a nested peek is up", () => {
    const { onClose } = setup({ suppressEscape: true });
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("renders a heading title for read-only surfaces", () => {
    setup({ titleAsHeading: true });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
  });

  it("commits title edits and tags nested panels", async () => {
    const user = userEvent.setup();
    const onCommitTitle = vi.fn();
    setup({ onCommitTitle, testId: "issue-preview-nested", stacked: true });
    expect(screen.getByTestId("issue-preview-nested")).toBeInTheDocument();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Renamed{Enter}");
    expect(onCommitTitle).toHaveBeenCalledWith("Renamed");
  });
});
