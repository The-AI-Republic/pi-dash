// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NewPasswordForm } from "./NewPasswordForm.js";

const STRONG = "Tq7!mZ2@kP9#xR";
const OTHER_STRONG = "Nb4@gH8$kL1!zQ";

function renderForm(props: Partial<React.ComponentProps<typeof NewPasswordForm>> = {}) {
  const onSubmit = vi.fn();
  render(<NewPasswordForm submitLabel="Do the thing" pending={false} onSubmit={onSubmit} {...props} />);
  return { onSubmit };
}

describe("NewPasswordForm", () => {
  it("locks the supplied address and leaves it out otherwise", () => {
    const { unmount } = render(
      <NewPasswordForm submitLabel="Do the thing" pending={false} onSubmit={vi.fn()} email="ada@example.com" />
    );
    const locked = screen.getByLabelText("Work email");
    expect(locked).toHaveValue("ada@example.com");
    expect(locked).toBeDisabled();
    unmount();

    render(<NewPasswordForm submitLabel="Do the thing" pending={false} onSubmit={vi.fn()} />);
    expect(screen.queryByLabelText("Work email")).toBeNull();
  });

  it("hints at the length rule first, then the mix rule, then goes quiet", async () => {
    const user = userEvent.setup();
    renderForm();
    const secret = screen.getByLabelText("New password");

    expect(screen.queryByText(/at least 8 characters|Weak password/)).toBeNull();
    await user.type(secret, "Ab1!");
    expect(screen.getByText("Use at least 8 characters.")).toBeInTheDocument();
    await user.clear(secret);
    await user.type(secret, "abcdefgh");
    expect(screen.getByText("Weak password — add mixed case, numbers, or symbols.")).toBeInTheDocument();
    await user.clear(secret);
    await user.type(secret, STRONG);
    expect(screen.queryByText(/at least 8 characters|Weak password/)).toBeNull();
  });

  it("shows the mismatch message on blur or once the confirmation catches up", async () => {
    const user = userEvent.setup();
    renderForm();
    await user.type(screen.getByLabelText("New password"), STRONG);
    const confirm = screen.getByLabelText("Confirm password");

    await user.type(confirm, STRONG.slice(0, 4));
    expect(screen.queryByText("The two passwords do not match.")).toBeNull();
    fireEvent.blur(confirm);
    expect(screen.getByText("The two passwords do not match.")).toBeInTheDocument();

    await user.clear(confirm);
    await user.type(confirm, OTHER_STRONG);
    expect(screen.getByText("The two passwords do not match.")).toBeInTheDocument();

    await user.clear(confirm);
    await user.type(confirm, STRONG);
    expect(screen.queryByText("The two passwords do not match.")).toBeNull();
  });

  it("toggles each field between hidden and shown independently", async () => {
    const user = userEvent.setup();
    renderForm();
    const secret = screen.getByLabelText("New password");
    const confirm = screen.getByLabelText("Confirm password");

    await user.click(screen.getByRole("button", { name: "Show new password" }));
    expect(secret).toHaveAttribute("type", "text");
    expect(confirm).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Hide new password" }));
    expect(secret).toHaveAttribute("type", "password");

    await user.click(screen.getByRole("button", { name: "Show confirmation" }));
    expect(confirm).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "Hide confirmation" }));
    expect(confirm).toHaveAttribute("type", "password");
  });

  it("submits only once the secret is strong and confirmed", async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderForm();
    const submit = screen.getByRole("button", { name: "Do the thing" });

    expect(submit).toBeDisabled();
    await user.type(screen.getByLabelText("New password"), "abcdefgh");
    await user.type(screen.getByLabelText("Confirm password"), "abcdefgh");
    expect(submit).toBeDisabled();

    await user.clear(screen.getByLabelText("New password"));
    await user.clear(screen.getByLabelText("Confirm password"));
    await user.type(screen.getByLabelText("New password"), STRONG);
    await user.type(screen.getByLabelText("Confirm password"), OTHER_STRONG);
    expect(submit).toBeDisabled();

    await user.clear(screen.getByLabelText("Confirm password"));
    await user.type(screen.getByLabelText("Confirm password"), STRONG);
    expect(submit).toBeEnabled();
    await user.click(submit);
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith(STRONG);
  });

  it("blocks resubmission while pending", () => {
    renderForm({ pending: true });
    expect(screen.getByRole("button", { name: "Do the thing" })).toBeDisabled();
  });
});
