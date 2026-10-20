// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { LINK_URL_REQUIRED, LinkDialog, type LinkDraft } from "./LinkDialog.js";
import { linkRow } from "./widgetTestUtils.js";

function renderDialog(link = null as ReturnType<typeof linkRow> | null, pending = false) {
  const onSubmit = vi.fn<(draft: LinkDraft) => void>();
  const onOpenChange = vi.fn();
  const view = render(
    <LinkDialog open onOpenChange={onOpenChange} link={link} pending={pending} onSubmit={onSubmit} />
  );
  return { onSubmit, onOpenChange, view };
}

describe("link dialog", () => {
  it("adds a link and prefixes a missing scheme", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("URL"), "example.com/page");
    await user.type(screen.getByLabelText("Display title (optional)"), "Page");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "http://example.com/page", title: "Page" });
  });

  it("keeps an existing http(s) scheme", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.type(screen.getByLabelText("URL"), "https://example.com/page");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "https://example.com/page", title: "" });
  });

  it("refuses an empty URL with an inline error", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(LINK_URL_REQUIRED);
    expect(screen.getByLabelText("URL")).toHaveAttribute("aria-invalid", "true");
  });

  it("clears the error on the next keystroke", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Add link" }));
    await user.type(screen.getByLabelText("URL"), "x");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("edits an existing link and prefills its draft", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog(linkRow({ title: "Old", url: "https://example.com/old" }));
    expect(screen.getByRole("dialog", { name: "Edit link" })).toBeInTheDocument();
    expect(screen.getByLabelText("URL")).toHaveValue("https://example.com/old");
    expect(screen.getByLabelText("Display title (optional)")).toHaveValue("Old");
    await user.click(screen.getByRole("button", { name: "Save link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "https://example.com/old", title: "Old" });
  });

  it("trims the title before submitting", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderDialog(null, false);
    await user.type(screen.getByLabelText("URL"), "example.com");
    await user.type(screen.getByLabelText("Display title (optional)"), "  spaced  ");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(onSubmit).toHaveBeenCalledWith({ url: "http://example.com", title: "spaced" });
  });

  it("marks the submit control busy while the write is pending", () => {
    renderDialog(null, true);
    expect(screen.getByRole("button", { name: "Add link" })).toBeDisabled();
  });

  it("closes through cancel", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});
