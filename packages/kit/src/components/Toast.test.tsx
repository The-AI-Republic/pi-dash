// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createKitToastManager, showToast, TOAST_COPIED_RESET_MS, ToastHost } from "./Toast";

function setup() {
  const manager = createKitToastManager();
  render(<ToastHost manager={manager} />);
  return manager;
}

describe("Toast", () => {
  it("shows a notification with title and description", async () => {
    const manager = setup();
    showToast(manager, { title: "Saved", description: "Issue updated", timeout: 0 });
    expect(await screen.findByText("Saved")).toBeInTheDocument();
    expect(screen.getByText("Issue updated")).toBeInTheDocument();
    expect(screen.getByTestId("toast-notification")).toContainElement(screen.getByText("Saved"));
  });

  it("exposes the stack under a stable test id for the parity driver", async () => {
    const manager = setup();
    expect(screen.getByTestId("toast-stack")).toBeInTheDocument();
    showToast(manager, { title: "Saved", timeout: 0 });
    await screen.findByText("Saved");
    expect(screen.getByTestId("toast-stack")).toHaveTextContent("Saved");
  });

  it("labels the stack as the Notifications region (CLOUD-024/025/026)", () => {
    setup();
    expect(screen.getByTestId("toast-stack")).toHaveAttribute("aria-label", "Notifications");
  });

  it("runs the action and dismisses", async () => {
    const user = userEvent.setup();
    const manager = setup();
    const onAction = vi.fn();
    showToast(manager, { title: "Saved", actions: [{ label: "Undo", run: onAction }], timeout: 0 });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
  });

  it("hides the close button until the toast expands, then dismisses", async () => {
    const user = userEvent.setup();
    const manager = setup();
    showToast(manager, { title: "Saved", timeout: 0 });
    await screen.findByText("Saved");
    // Collapsed stack: the close control stays out of the a11y tree.
    expect(screen.queryByRole("button", { name: "Dismiss notification" })).not.toBeInTheDocument();
    await user.hover(screen.getByRole("dialog", { name: "Saved" }));
    await user.click(screen.getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
  });
});

describe("Toast inline actions", () => {
  it("renders an anchor action that opens in a new tab", async () => {
    const manager = setup();
    showToast(manager, {
      title: "Saved",
      timeout: 0,
      actions: [{ label: "View work item", href: "/acme/browse/WEB-1/" }],
    });
    await screen.findByText("Saved");
    const link = screen.getByRole("link", { name: "View work item" });
    expect(link).toHaveAttribute("href", "/acme/browse/WEB-1/");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
  });

  it("runs a copy action and shows the confirmation until the timer resets it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      const manager = setup();
      const run = vi.fn();
      showToast(manager, {
        title: "Saved",
        timeout: 0,
        actions: [{ label: "Copy link", run, confirmLabel: "Copied!" }],
      });
      await screen.findByText("Saved");
      await user.click(screen.getByRole("button", { name: "Copy link" }));
      expect(run).toHaveBeenCalledTimes(1);
      expect(screen.getByRole("button", { name: "Copied!" })).toBeInTheDocument();
      // The confirmation is brief and the toast stays up for another try.
      vi.advanceTimersByTime(TOAST_COPIED_RESET_MS);
      expect(await screen.findByRole("button", { name: "Copy link" })).toBeInTheDocument();
      expect(screen.getByText("Saved")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the label when a copy action reports a failed write", async () => {
    const user = userEvent.setup();
    const manager = setup();
    showToast(manager, {
      title: "Saved",
      timeout: 0,
      actions: [{ label: "Copy link", run: () => false, confirmLabel: "Copied!" }],
    });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("button", { name: "Copy link" }));
    expect(screen.getByRole("button", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copied!" })).not.toBeInTheDocument();
  });

  it("waits for an async copy action before confirming", async () => {
    const user = userEvent.setup();
    const manager = setup();
    showToast(manager, {
      title: "Saved",
      timeout: 0,
      actions: [{ label: "Copy link", run: async () => true, confirmLabel: "Copied!" }],
    });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("button", { name: "Copy link" }));
    expect(await screen.findByRole("button", { name: "Copied!" })).toBeInTheDocument();
  });

  it("dismisses when a plain action without a run is chosen", async () => {
    const user = userEvent.setup();
    const manager = setup();
    showToast(manager, { title: "Moved", timeout: 0, actions: [{ label: "Open" }] });
    await screen.findByText("Moved");
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.queryByText("Moved")).not.toBeInTheDocument();
  });

  it("runs a plain action and dismisses even when it resolves later", async () => {
    const user = userEvent.setup();
    const manager = setup();
    const run = vi.fn(async () => undefined);
    showToast(manager, { title: "Moved", actions: [{ label: "Open", run }], timeout: 0 });
    await screen.findByText("Moved");
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(run).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByText("Moved")).not.toBeInTheDocument());
  });

  it("runs an inline copy action, swaps to its confirmation, then restores the label", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      const manager = setup();
      const run = vi.fn(async () => true);
      showToast(manager, {
        title: "Saved",
        timeout: 0,
        actions: [{ label: "Copy link", run, confirmLabel: "Copied!" }],
      });
      await screen.findByText("Saved");
      await user.click(screen.getByRole("button", { name: "Copy link" }));
      expect(await screen.findByRole("button", { name: "Copied!" })).toBeInTheDocument();
      vi.advanceTimersByTime(TOAST_COPIED_RESET_MS);
      expect(await screen.findByRole("button", { name: "Copy link" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("runs an anchor action's own callback and dismisses the notice", async () => {
    const user = userEvent.setup();
    const manager = setup();
    const run = vi.fn();
    showToast(manager, {
      title: "Saved",
      timeout: 0,
      actions: [{ label: "View work item", href: "/acme/browse/WEB-1/", run }],
    });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("link", { name: "View work item" }));
    expect(run).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByText("Saved")).not.toBeInTheDocument());
  });

  it("keeps the action row until the copy reports success", async () => {
    const user = userEvent.setup();
    const manager = setup();
    const run = vi.fn(async () => false);
    showToast(manager, { title: "Saved", actions: [{ label: "Copy link", run, confirmLabel: "Copied!" }], timeout: 0 });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("button", { name: "Copy link" }));
    expect(run).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByText("Copied!")).not.toBeInTheDocument();
  });
});
