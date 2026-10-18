// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row-action confirmation (NEWFRONT-540, RUN-010). Both actions share one
// dialog: heading, consequence copy in a block directly below the heading,
// a cancel and a confirm named after the action. In flight, every control
// that could dismiss it is inert.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  DELETE_RUNNER_BODY,
  DELETE_RUNNER_TITLE,
  REVOKE_RUNNER_BODY,
  REVOKE_RUNNER_TITLE,
  RunnerConfirmDialog,
  runnerConfirmCopy,
} from "./RunnerConfirmDialog.js";

function setup(overrides: Partial<React.ComponentProps<typeof RunnerConfirmDialog>> = {}): {
  onCancel: ReturnType<typeof vi.fn>;
  onConfirm: ReturnType<typeof vi.fn>;
} {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(
    <RunnerConfirmDialog
      open
      action="delete"
      runnerName="alpha-runner"
      pending={false}
      onCancel={onCancel}
      onConfirm={onConfirm}
      {...overrides}
    />
  );
  return { onCancel, onConfirm };
}

describe("runnerConfirmCopy", () => {
  it("pins the delete consequences", () => {
    const copy = runnerConfirmCopy("delete");
    expect(copy.title).toBe(DELETE_RUNNER_TITLE);
    expect(copy.body).toBe(DELETE_RUNNER_BODY);
    expect(copy.body).toContain("Historic runs are preserved");
    expect(copy.body).toContain("does not uninstall");
    expect(copy.confirm).toBe("Delete");
  });

  it("pins the revoke consequences", () => {
    const copy = runnerConfirmCopy("revoke");
    expect(copy.title).toBe(REVOKE_RUNNER_TITLE);
    expect(copy.body).toBe(REVOKE_RUNNER_BODY);
    expect(copy.body).toContain("credentials are invalidated");
    expect(copy.body).toContain("row stays in the list");
    expect(copy.confirm).toBe("Revoke");
  });
});

describe("RunnerConfirmDialog", () => {
  it("renders the heading, copy block and the runner's name", () => {
    setup();
    const dialog = screen.getByRole("dialog", { name: DELETE_RUNNER_TITLE });
    expect(dialog).toBeInTheDocument();
    // The copy block sits directly below the heading.
    const heading = Array.from(dialog.querySelectorAll("h1, h2, h3, h4")).find(
      (node) => node.textContent === DELETE_RUNNER_TITLE
    );
    expect(heading?.textContent).toBe(DELETE_RUNNER_TITLE);
    expect(heading?.nextElementSibling?.textContent).toBe(DELETE_RUNNER_BODY);
    expect(screen.getByText("alpha-runner")).toBeInTheDocument();
  });

  it("confirms through the action button", async () => {
    const { onConfirm, onCancel } = setup({ action: "revoke" });
    await userEvent.click(screen.getByRole("button", { name: "Revoke" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("cancels through the cancel button", async () => {
    const { onCancel, onConfirm } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("dismisses on Escape when idle", async () => {
    const { onCancel } = setup();
    await userEvent.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("blocks every dismissal path while the write is in flight", async () => {
    const { onCancel, onConfirm } = setup({ pending: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    const confirm = screen.getByRole("button", { name: "Delete" });
    expect(confirm).toBeDisabled();
    expect(confirm).toHaveAttribute("aria-busy", "true");
    // No close button either: the dialog is non-dismissable.
    expect(screen.queryByRole("button", { name: "Close dialog" })).toBeNull();
    await userEvent.keyboard("{Escape}");
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
