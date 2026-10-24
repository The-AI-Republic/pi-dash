// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makeModuleLink } from "../../testing.js";
import { ModuleLinkDialog, type ModuleLinkDraft } from "./ModuleLinkDialog.js";

function renderDialog(link: React.ComponentProps<typeof ModuleLinkDialog>["link"] = null) {
  const onSubmit = vi.fn();
  const onOpenChange = vi.fn();
  render(<ModuleLinkDialog open onOpenChange={onOpenChange} link={link} pending={false} onSubmit={onSubmit} />);
  return { onSubmit: onSubmit as (draft: ModuleLinkDraft) => void, onOpenChange };
}

describe("ModuleLinkDialog", () => {
  it("adds with empty fields under the Add title", () => {
    renderDialog();
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    expect(screen.getByLabelText("URL")).toHaveValue("");
    expect(screen.getByLabelText("Display title")).toHaveValue("");
    expect(screen.getByText("Optional")).toBeInTheDocument();
  });

  it("edits with the row prefilled under the Update title", () => {
    renderDialog(makeModuleLink({ title: "Launch plan", url: "https://docs.example.com/launch" }));
    expect(screen.getByRole("dialog", { name: "Update link" })).toBeInTheDocument();
    expect(screen.getByLabelText("URL")).toHaveValue("https://docs.example.com/launch");
    expect(screen.getByLabelText("Display title")).toHaveValue("Launch plan");
  });

  it("prefills an empty title for an untitled row", () => {
    renderDialog(makeModuleLink({ title: null }));
    expect(screen.getByLabelText("Display title")).toHaveValue("");
  });

  it("marks the URL invalid and submits nothing when empty", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.click(screen.getByTestId("modules-link-submit"));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByLabelText("URL")).toHaveAttribute("aria-invalid", "true");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
  });

  it("rejects a whitespace-only URL the same way", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.type(screen.getByLabelText("URL"), "   ");
    await user.click(screen.getByTestId("modules-link-submit"));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByLabelText("URL")).toHaveAttribute("aria-invalid", "true");
  });

  it("clears the invalid mark while typing and submits the repair", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.click(screen.getByTestId("modules-link-submit"));
    expect(screen.getByLabelText("URL")).toHaveAttribute("aria-invalid", "true");
    await user.type(screen.getByLabelText("URL"), "example.com");
    expect(screen.getByLabelText("URL")).toHaveAttribute("aria-invalid", "false");
    await user.type(screen.getByLabelText("Display title"), "  Docs  ");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "http://example.com", title: "Docs" });
  });

  it("keeps an explicit https scheme and nulls a blank title", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.type(screen.getByLabelText("URL"), "https://plain.example.com");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "https://plain.example.com", title: null });
  });

  it("discards the draft on Cancel", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = renderDialog();
    await user.type(screen.getByLabelText("URL"), "example.com");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("renders nothing while closed", () => {
    render(
      <ModuleLinkDialog
        open={false}
        onOpenChange={() => undefined}
        link={null}
        pending={false}
        onSubmit={() => undefined}
      />
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
