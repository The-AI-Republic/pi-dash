// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Two-step email change (parity/specs/profile.md §6, PROF-013): shape and
// availability before the code is mailed, the locked address on step two,
// inline refusals and the hand-off after a successful move.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  checkTaken: { mutateAsync: vi.fn(), isPending: false },
  sendCode: { mutateAsync: vi.fn(), isPending: false },
  confirm: { mutateAsync: vi.fn(), isPending: false },
}));

vi.mock("../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api.js")>()),
  useCheckEmailTaken: () => harness.checkTaken,
  useGenerateEmailCode: () => harness.sendCode,
  useConfirmEmailChange: () => harness.confirm,
}));

import { EMAIL_MESSAGES } from "../lib/email.js";
import { ChangeEmailDialog } from "./ChangeEmailDialog.js";

function setup(overrides: Partial<React.ComponentProps<typeof ChangeEmailDialog>> = {}) {
  const handlers = { onOpenChange: vi.fn(), onChanged: vi.fn() };
  const view = render(<ChangeEmailDialog open currentEmail="ada@example.com" {...handlers} {...overrides} />);
  return { ...handlers, ...view };
}

async function typeEmail(value: string): Promise<void> {
  const field = screen.getByLabelText("New email");
  await userEvent.clear(field);
  if (value !== "") await userEvent.type(field, value);
}

beforeEach(() => {
  harness.checkTaken.mutateAsync.mockReset().mockResolvedValue(false);
  harness.sendCode.mutateAsync.mockReset().mockResolvedValue({});
  harness.confirm.mutateAsync.mockReset().mockResolvedValue({});
  harness.checkTaken.isPending = false;
  harness.sendCode.isPending = false;
  harness.confirm.isPending = false;
});

describe("ChangeEmailDialog", () => {
  it("explains the flow and starts on the address step", () => {
    setup();
    const dialog = screen.getByRole("dialog", { name: "Change email" });
    expect(dialog).toHaveTextContent("Enter a new email address to receive a verification link.");
    expect(screen.getByLabelText("New email")).toHaveAttribute("placeholder", "Enter your email");
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("requires the new address", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText(EMAIL_MESSAGES.required)).toBeInTheDocument();
    expect(harness.checkTaken.mutateAsync).not.toHaveBeenCalled();
  });

  it("refuses a malformed address", async () => {
    setup();
    await typeEmail("not-an-address");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText(EMAIL_MESSAGES.invalid)).toBeInTheDocument();
    expect(harness.checkTaken.mutateAsync).not.toHaveBeenCalled();
  });

  it("refuses the address the account already uses, whatever its casing", async () => {
    setup();
    await typeEmail("ADA@example.com ");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText(EMAIL_MESSAGES.unchanged)).toBeInTheDocument();
  });

  it("stops on a taken address without mailing a code", async () => {
    harness.checkTaken.mutateAsync.mockResolvedValue(true);
    setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByText(EMAIL_MESSAGES.taken)).toBeInTheDocument());
    expect(harness.sendCode.mutateAsync).not.toHaveBeenCalled();
  });

  it("reports a failed availability check inline", async () => {
    harness.checkTaken.mutateAsync.mockRejectedValue({ message: "Address lookup failed" });
    setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByText("Address lookup failed")).toBeInTheDocument());
  });

  it("reports a failed code send inline", async () => {
    harness.sendCode.mutateAsync.mockRejectedValue({ body: { error: "Already exists for another member" } });
    setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByText(EMAIL_MESSAGES.taken)).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("moves to the code step with the address locked", async () => {
    setup();
    await typeEmail("  Grace@Example.com ");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    expect(harness.checkTaken.mutateAsync).toHaveBeenCalledWith("grace@example.com");
    expect(harness.sendCode.mutateAsync).toHaveBeenCalledWith("grace@example.com");
    expect(screen.getByLabelText("New email")).toBeDisabled();
    expect(screen.getByLabelText("Unique code")).toHaveAttribute("placeholder", "123456");
    expect(screen.getByText(EMAIL_MESSAGES.sent)).toBeInTheDocument();
  });

  it("keeps every keystroke when the dialog's initial focus frame arrives late", async () => {
    // Base UI moves initial focus on the next animation frame. On a busy
    // host that frame can land between keystrokes; a dialog that relies on
    // the default target then moves focus to its close control mid-word and
    // the address is truncated. The address field must own focus from open.
    const raf = vi
      .spyOn(globalThis, "requestAnimationFrame")
      .mockImplementation(
        (callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 50) as unknown as number
      );
    try {
      setup();
      const field = screen.getByLabelText("New email");
      await waitFor(() => expect(field).toHaveFocus());
      await typeEmail("grace@example.com");
      expect(field).toHaveValue("grace@example.com");
      expect(field).toHaveFocus();
    } finally {
      raf.mockRestore();
    }
  });

  it("moves focus to the code field when the code step opens", async () => {
    setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    await waitFor(() => expect(screen.getByLabelText("Unique code")).toHaveFocus());
  });

  it("requires a code before verifying", async () => {
    setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(screen.getByText(EMAIL_MESSAGES.codeRequired)).toBeInTheDocument();
    expect(harness.confirm.mutateAsync).not.toHaveBeenCalled();
  });

  it("keeps a wrong code inline on the code field", async () => {
    harness.confirm.mutateAsync.mockRejectedValue({ body: { error: EMAIL_MESSAGES.codeInvalid } });
    const { onChanged } = setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    await userEvent.type(screen.getByLabelText("Unique code"), "000000");
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(screen.getByText(EMAIL_MESSAGES.codeInvalid)).toBeInTheDocument());
    expect(harness.confirm.mutateAsync).toHaveBeenCalledWith({ email: "grace@example.com", code: "000000" });
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("clears an inline refusal once the member edits the field", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText(EMAIL_MESSAGES.required)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("New email"), "g");
    expect(screen.queryByText(EMAIL_MESSAGES.required)).not.toBeInTheDocument();
  });

  it("hands off after a verified move", async () => {
    const { onChanged } = setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    await userEvent.type(screen.getByLabelText("Unique code"), " 123456 ");
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(harness.confirm.mutateAsync).toHaveBeenCalledWith({ email: "grace@example.com", code: "123456" });
  });

  it("holds the buttons while a step is in flight", async () => {
    harness.checkTaken.isPending = true;
    setup();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("ignores a submit while a step is in flight", async () => {
    const { container } = setup();
    harness.checkTaken.isPending = true;
    container.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(harness.checkTaken.mutateAsync).not.toHaveBeenCalled();
  });

  it("resets to the first step when it closes and reopens", async () => {
    const view = setup();
    await typeEmail("grace@example.com");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument());
    view.rerender(
      <ChangeEmailDialog open={false} currentEmail="ada@example.com" onOpenChange={vi.fn()} onChanged={vi.fn()} />
    );
    view.rerender(<ChangeEmailDialog open currentEmail="ada@example.com" onOpenChange={vi.fn()} onChanged={vi.fn()} />);
    expect(screen.getByLabelText("New email")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });
});
