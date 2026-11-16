// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Security tab (parity/specs/profile.md §6, PROF-015): the strength meter,
// the mismatch and same-as-old notes, per-field reveal, the submit gate,
// and the server's weak-secret refusal on the field.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  me: undefined as Record<string, unknown> | undefined,
  change: { mutate: vi.fn(), isPending: false },
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: () => ({ data: harness.me }),
}));

vi.mock("../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api.js")>()),
  useChangePassword: () => harness.change,
}));

import { SecuritySettingsTab } from "./SecuritySettingsTab.js";

const SET_ACCOUNT = { is_password_autoset: false };

function renderTab(): void {
  render(<SecuritySettingsTab />);
}

function submit(): void {
  screen.getByTestId("security-settings-form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

beforeEach(() => {
  harness.me = SET_ACCOUNT;
  harness.change.mutate = vi.fn();
  harness.change.isPending = false;
});

describe("SecuritySettingsTab", () => {
  it("holds a loading frame until the member row arrives", () => {
    harness.me = undefined;
    renderTab();
    expect(screen.getByTestId("security-settings-loading")).toHaveAttribute("aria-busy", "true");
  });

  it("asks a password account for the current secret", () => {
    renderTab();
    expect(screen.getByRole("heading", { name: "Change password" })).toBeInTheDocument();
    expect(screen.getByLabelText("Current password")).toBeInTheDocument();
    expect(screen.getByLabelText("New password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();
  });

  it("skips the current field for provider accounts", () => {
    harness.me = { is_password_autoset: true };
    renderTab();
    expect(screen.queryByLabelText("Current password")).not.toBeInTheDocument();
    expect(screen.getByLabelText("New password")).toBeInTheDocument();
  });

  it("lists the unmet strength rules as a weak guess is typed", async () => {
    const user = userEvent.setup();
    renderTab();
    expect(screen.queryByTestId("password-strength")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("New password"), "abc");
    const meter = screen.getByTestId("password-strength");
    expect(meter).toHaveTextContent("Min 8 characters");
    expect(meter).toHaveTextContent("An upper-case letter");
    expect(meter).toHaveTextContent("A number");
    expect(meter).toHaveTextContent("A symbol");
  });

  it("hides the meter once every rule holds", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("New password"), "Str0ng!pass");
    expect(screen.queryByTestId("password-strength")).not.toBeInTheDocument();
  });

  it("waits for a blur before reporting a shorter confirmation", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "New");
    expect(screen.queryByText("Passwords don't match")).not.toBeInTheDocument();
    await user.tab();
    expect(screen.getByText("Passwords don't match")).toBeInTheDocument();
  });

  it("reports a mismatch as soon as the confirmation is as long as the guess", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("New password"), "NewS");
    await user.type(screen.getByLabelText("Confirm password"), "NewT");
    expect(screen.getByText("Passwords don't match")).toBeInTheDocument();
  });

  it("notes a guess equal to the current secret once the new field loses focus", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "Str0ng!pass");
    await user.type(screen.getByLabelText("New password"), "Str0ng!pass");
    expect(screen.queryByText("New password must be different from old password")).not.toBeInTheDocument();
    await user.tab();
    expect(screen.getByText("New password must be different from old password")).toBeInTheDocument();
  });

  it("reveals each field on its own", async () => {
    const user = userEvent.setup();
    renderTab();
    const next = screen.getByLabelText("New password");
    expect(next).toHaveAttribute("type", "password");
    const reveal = screen.getByRole("button", { name: "Show next password" });
    await user.click(reveal);
    expect(screen.getByLabelText("New password")).toHaveAttribute("type", "text");
    expect(screen.getByLabelText("Current password")).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Hide next password" }));
    expect(screen.getByLabelText("New password")).toHaveAttribute("type", "password");
  });

  it("gates submit on a strong, matching, changed guess", async () => {
    const user = userEvent.setup();
    renderTab();
    const button = () => screen.getByRole("button", { name: /Change password|Changing password/ });
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "weak");
    await user.type(screen.getByLabelText("Confirm password"), "weak");
    expect(button()).toBeDisabled();
    await user.clear(screen.getByLabelText("New password"));
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    expect(button()).toBeDisabled();
    await user.clear(screen.getByLabelText("Confirm password"));
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    expect(button()).toBeEnabled();
  });

  it("does not post an empty form even if submit is forced", () => {
    renderTab();
    submit();
    expect(harness.change.mutate).not.toHaveBeenCalled();
  });

  it("posts the old and new secrets for a password account", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(harness.change.mutate).toHaveBeenCalledWith(
      { oldPassword: "OldStr0ng!", newPassword: "NewStr0ng!" },
      expect.anything()
    );
  });

  it("omits the old secret for a provider account", async () => {
    harness.me = { is_password_autoset: true };
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(harness.change.mutate).toHaveBeenCalledWith({ newPassword: "NewStr0ng!" }, expect.anything());
  });

  it("shows the changing label while the write runs", () => {
    harness.change.isPending = true;
    renderTab();
    expect(screen.getByRole("button", { name: "Changing password" })).toBeDisabled();
  });

  it("puts a server weak-secret refusal on the new field", async () => {
    harness.change.mutate.mockImplementation((_input: unknown, options: { onError: (error: unknown) => void }) => {
      options.onError({ code: "5021" });
    });
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(screen.getByText("Please use a stronger password.")).toBeInTheDocument();
  });

  it("puts a missing-secret refusal on the current field", async () => {
    harness.change.mutate.mockImplementation((_input: unknown, options: { onError: (error: unknown) => void }) => {
      options.onError({ code: 5138 });
    });
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(screen.queryByText("Please use a stronger password.")).not.toBeInTheDocument();
    expect(screen.getByText("Enter your current and new password.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Current password"), "x");
    await waitFor(() => expect(screen.queryByText("Enter your current and new password.")).not.toBeInTheDocument());
  });

  it("clears the form and the field error after a successful change", async () => {
    harness.change.mutate.mockImplementation(
      (_input: unknown, options: { onError: (error: unknown) => void; onSuccess: () => void }) => {
        options.onError({ code: "5021" });
        options.onSuccess();
      }
    );
    const user = userEvent.setup();
    renderTab();
    await user.type(screen.getByLabelText("Current password"), "OldStr0ng!");
    await user.type(screen.getByLabelText("New password"), "NewStr0ng!");
    await user.type(screen.getByLabelText("Confirm password"), "NewStr0ng!");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    await waitFor(() => expect(screen.getByLabelText("New password")).toHaveValue(""));
    expect(screen.getByLabelText("Current password")).toHaveValue("");
    expect(screen.getByLabelText("Confirm password")).toHaveValue("");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();
  });
});
