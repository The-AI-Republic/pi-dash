// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-key model-ID editor (NEWPIDASH-394, design sections 7.2 and 7.3):
// per-ID status and failure detail, test progress, add/remove, the
// last-ID removal confirmation, and the "Save anyway" retry slot.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { lastModelRemoveBody } from "../lib/copy.js";
import { ModelIdEditor } from "./ModelIdEditor.js";

function setup(overrides: Partial<React.ComponentProps<typeof ModelIdEditor>> = {}) {
  const props: React.ComponentProps<typeof ModelIdEditor> = {
    ids: ["gpt-6", "gpt-6.2"],
    verifiedAt: { "gpt-6": "2026-11-23T00:00:00Z" },
    failures: { "gpt-6.2": "Model not accepted by the provider." },
    pendingIds: [],
    disabled: false,
    testable: true,
    confirmLastRemove: true,
    keyLabel: "BYOK:openrouter.ai",
    onAdd: vi.fn(),
    onRemove: vi.fn(),
    onTest: vi.fn(),
    onSaveAnyway: null,
    ...overrides,
  };
  render(<ModelIdEditor {...props} />);
  return props;
}

function row(model: string): HTMLElement {
  const rows = screen.getAllByTestId("byok-model-row");
  const found = rows.find((row) => within(row).queryByText(model) !== null);
  if (found === undefined) throw new Error(`no editor row for ${model}`);
  return found;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ModelIdEditor", () => {
  it("marks each ID verified, failed or not verified with the provider message", () => {
    setup({ ids: ["gpt-6", "gpt-6.2", "fresh"], verifiedAt: { "gpt-6": "2026-11-23T00:00:00Z" } });
    expect(within(row("gpt-6")).getByTestId("byok-model-status")).toHaveTextContent("Verified");
    expect(within(row("gpt-6.2")).getByTestId("byok-model-status")).toHaveTextContent("Failed");
    expect(within(row("gpt-6.2")).getByTestId("byok-model-detail")).toHaveTextContent(
      "Model not accepted by the provider."
    );
    expect(within(row("fresh")).getByTestId("byok-model-status")).toHaveTextContent("Not verified");
  });

  it("shows test progress on the probing row instead of its status", () => {
    setup({ pendingIds: ["gpt-6.2"] });
    expect(within(row("gpt-6.2")).queryByTestId("byok-model-status")).toBeNull();
    expect(within(row("gpt-6.2")).getByText("Testing…")).toBeInTheDocument();
  });

  it("adds a trimmed ID and ignores empties and duplicates", async () => {
    const user = userEvent.setup();
    const props = setup();
    const input = screen.getByLabelText("Add a model ID this key serves");
    await user.type(input, "   {Enter}");
    expect(props.onAdd).not.toHaveBeenCalled();
    await user.clear(input);
    await user.type(input, "gpt-6{Enter}");
    expect(props.onAdd).not.toHaveBeenCalled();
    await user.clear(input);
    await user.type(input, "gpt-6.3{Enter}");
    expect(props.onAdd).toHaveBeenCalledWith("gpt-6.3");
  });

  it("tests and removes a non-last ID at once", async () => {
    const user = userEvent.setup();
    const props = setup();
    await user.click(within(row("gpt-6")).getByRole("button", { name: "Test gpt-6" }));
    expect(props.onTest).toHaveBeenCalledWith("gpt-6");
    await user.click(within(row("gpt-6")).getByRole("button", { name: "Remove gpt-6" }));
    expect(props.onRemove).toHaveBeenCalledWith("gpt-6");
  });

  it("asks for confirmation before removing the last ID, then removes", async () => {
    const user = userEvent.setup();
    const props = setup({ ids: ["gpt-6"], verifiedAt: {}, failures: {} });
    await user.click(screen.getByRole("button", { name: "Remove gpt-6" }));
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(screen.getByText(lastModelRemoveBody("gpt-6", "BYOK:openrouter.ai"))).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove both" }));
    expect(props.onRemove).toHaveBeenCalledWith("gpt-6");
  });

  it("removes the last draft ID at once when confirmation is off", async () => {
    const user = userEvent.setup();
    const props = setup({ ids: ["gpt-6"], verifiedAt: {}, failures: {}, confirmLastRemove: false });
    await user.click(screen.getByRole("button", { name: "Remove gpt-6" }));
    expect(props.onRemove).toHaveBeenCalledWith("gpt-6");
    expect(screen.queryByRole("button", { name: "Remove both" })).toBeNull();
  });

  it("cancels the last-ID prompt without removing", async () => {
    const user = userEvent.setup();
    const props = setup({ ids: ["gpt-6"], verifiedAt: {}, failures: {} });
    await user.click(screen.getByRole("button", { name: "Remove gpt-6" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Remove both" })).toBeNull();
  });

  it("hides the Test buttons where nothing is stored to test yet", () => {
    setup({ testable: false });
    expect(screen.queryByRole("button", { name: "Test gpt-6" })).toBeNull();
    expect(screen.getByRole("button", { name: "Remove gpt-6" })).toBeInTheDocument();
  });

  it("offers the Save-anyway retry while the caller offers it", async () => {
    const user = userEvent.setup();
    const onSaveAnyway = vi.fn();
    setup({ onSaveAnyway });
    await user.click(screen.getByRole("button", { name: "Save anyway" }));
    expect(onSaveAnyway).toHaveBeenCalled();
  });

  it("hides the Save-anyway retry when the caller offers none", () => {
    setup({ onSaveAnyway: null });
    expect(screen.queryByRole("button", { name: "Save anyway" })).toBeNull();
  });

  it("disables every control while a save runs", () => {
    setup({ disabled: true });
    expect(screen.getByRole("button", { name: "Test gpt-6" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove gpt-6" })).toBeDisabled();
    expect(screen.getByLabelText("Add a model ID this key serves")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();
  });
});
