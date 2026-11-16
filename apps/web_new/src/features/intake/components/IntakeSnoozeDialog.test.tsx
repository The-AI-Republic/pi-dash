// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snooze dialog (INT-022): past days disabled, the current snooze day
// as the start, pick-and-confirm, and the locked pending controls.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { TransportResponse } from "@pidash/api-client";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SNOOZE_DIALOG_TITLE, IntakeSnoozeDialog } from "./IntakeSnoozeDialog.js";

function isoDay(offset: number): string {
  const at = new Date();
  at.setDate(at.getDate() + offset);
  const month = String(at.getMonth() + 1).padStart(2, "0");
  const day = String(at.getDate()).padStart(2, "0");
  return `${at.getFullYear()}-${month}-${day}`;
}

function stubTransport(): void {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    // The profile read fails: the calendar falls back to the default week start.
    return { status: 500, url, headers: { get: () => null }, text: async () => '{"error":"boom"}' };
  }) as never as (url: string) => Promise<TransportResponse>);
}

function setup(props: Partial<React.ComponentProps<typeof IntakeSnoozeDialog>> = {}) {
  stubTransport();
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const renderWith = (next: Partial<React.ComponentProps<typeof IntakeSnoozeDialog>>) => (
    <QueryClientProvider client={queryClient}>
      <IntakeSnoozeDialog
        open
        pending={false}
        initialDay={null}
        onCancel={onCancel}
        onConfirm={onConfirm}
        {...props}
        {...next}
      />
    </QueryClientProvider>
  );
  const view = render(renderWith({}));
  return { onCancel, onConfirm, view, renderWith };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("IntakeSnoozeDialog", () => {
  it("disables past days and enables today and future ones", async () => {
    setup();
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    const cell = (day: string) => document.querySelector(`[data-day="${day}"]`) as HTMLButtonElement | null;
    expect(cell(isoDay(-1))).not.toBeNull();
    expect(cell(isoDay(-1))).toBeDisabled();
    expect(cell(isoDay(0))).not.toBeNull();
    expect(cell(isoDay(0))).toBeEnabled();
    expect(cell(isoDay(1))).toBeEnabled();
  });

  it("starts on the request's current snooze day", async () => {
    const future = isoDay(60);
    setup({ initialDay: future });
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    expect(document.querySelector(`[data-day="${future}"]`)).not.toBeNull();
  });

  it("confirms the picked day", async () => {
    const user = userEvent.setup();
    const tomorrow = isoDay(1);
    const { onConfirm } = setup();
    await waitFor(() => expect(screen.getByRole("button", { name: "Snooze" })).toBeDisabled());
    const cell = document.querySelector(`[data-day="${tomorrow}"]`) as HTMLElement;
    await user.click(cell);
    await user.click(screen.getByRole("button", { name: "Snooze" }));
    expect(onConfirm).toHaveBeenCalledWith(tomorrow);
  });

  it("treats cancel, the close control and Escape as cancel", async () => {
    const user = userEvent.setup();
    const h = setup();
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onCancel).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(h.onCancel).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(h.onCancel).toHaveBeenCalledTimes(3);
    expect(h.onConfirm).not.toHaveBeenCalled();
  });

  it("resets the pick when reopened", async () => {
    const user = userEvent.setup();
    const tomorrow = isoDay(1);
    const { view, renderWith } = setup();
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    const cell = document.querySelector(`[data-day="${tomorrow}"]`) as HTMLElement;
    await user.click(cell);
    expect(screen.getByRole("button", { name: "Snooze" })).toBeEnabled();
    view.rerender(renderWith({ open: false }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    view.rerender(renderWith({ open: true }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Snooze" })).toBeDisabled();
  });

  it("locks the controls while the write runs", async () => {
    setup({ pending: true, initialDay: isoDay(2) });
    await waitFor(() => expect(screen.getByRole("dialog", { name: SNOOZE_DIALOG_TITLE })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Snooze" })).toBeDisabled();
  });

  it("stays closed when shut", () => {
    setup({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
