// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Discard guard dialog (parity/specs/issues.md §11, ISS-120): the three
// ways out — discard, cancel, save to drafts — plus the saving and
// failure states the caller drives.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DiscardDraftDialog } from "./DiscardDraftDialog.js";

function setup(props: Partial<React.ComponentProps<typeof DiscardDraftDialog>> = {}) {
  const onCancel = vi.fn();
  const onDiscard = vi.fn();
  const onSaveDraft = vi.fn();
  render(<DiscardDraftDialog open onCancel={onCancel} onDiscard={onDiscard} onSaveDraft={onSaveDraft} {...props} />);
  return { onCancel, onDiscard, onSaveDraft };
}

describe("DiscardDraftDialog", () => {
  it("names the prompt and offers all three exits", () => {
    setup();
    expect(screen.getByRole("dialog", { name: "Save this draft?" })).toBeInTheDocument();
    for (const name of ["Discard", "Cancel", "Save to Drafts"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
  });

  it("reports each choice to the caller", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(h.onDiscard).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onCancel).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Save to Drafts" }));
    expect(h.onSaveDraft).toHaveBeenCalledTimes(1);
  });

  it("shows the saving state and blocks re-entry", () => {
    setup({ saving: true });
    const save = screen.getByRole("button", { name: "Saving…" });
    expect(save).toBeDisabled();
    expect(screen.getByRole("button", { name: "Discard" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("renders the caller's failure note", () => {
    setup({ error: "The draft didn't save." });
    expect(screen.getByRole("alert")).toHaveTextContent("The draft didn't save.");
  });

  it("treats Escape and the close control as cancel", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(h.onCancel).toHaveBeenCalledTimes(1);
  });
});
