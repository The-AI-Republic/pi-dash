// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header lock control (parity/specs/pages.md §2.3 and §7.3;
// PAGE-058): the resting/locked labels, the brief transitional phase on
// unlock, the optimistic flip with rollback on failure, and the busy
// state while a write flies.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const lockApi = vi.hoisted(() => ({ lock: vi.fn(), unlock: vi.fn() }));
const notices = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  lockPage: lockApi.lock,
  unlockPage: lockApi.unlock,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: () => ({ settle: vi.fn() }),
  notifySuccess: notices.success,
  notifyError: notices.error,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import {
  PageDetailLockControl,
  PAGE_LOCK_LABEL,
  PAGE_LOCKED_LABEL,
  PAGE_UNLOCK_TRANSITION_MS,
  PAGE_UNLOCKED_LABEL,
} from "./PageDetailLockControl.js";

function viewFor(client: QueryClient, locked: boolean): React.ReactElement {
  return (
    <QueryClientProvider client={client}>
      <PageDetailLockControl
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ is_locked: locked })}
      />
    </QueryClientProvider>
  );
}

function renderControl(locked: boolean): { client: QueryClient; relock: (next: boolean) => void } {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const view = render(viewFor(client, locked));
  return { client, relock: (next: boolean) => view.rerender(viewFor(client, next)) };
}

/** Flush the mutation microtasks and the transition timers together. */
async function settle(ms = 0): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  lockApi.lock.mockResolvedValue(undefined);
  lockApi.unlock.mockResolvedValue(undefined);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  vi.useRealTimers();
  resetAppClient();
});

describe("PageDetailLockControl", () => {
  it("locks a resting page on press", async () => {
    renderControl(false);
    const control = screen.getByRole("button", { name: PAGE_LOCK_LABEL });
    expect(control.querySelector("svg.lucide-lock-open")).not.toBeNull();

    fireEvent.click(control);
    await settle();
    expect(lockApi.lock).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(lockApi.unlock).not.toHaveBeenCalled();
  });

  it("unlocks a locked page through the brief transitional phase", async () => {
    const { relock } = renderControl(true);
    const control = screen.getByRole("button", { name: PAGE_LOCKED_LABEL });
    expect(control.querySelector("svg.lucide-lock")).not.toBeNull();

    fireEvent.click(control);
    await settle();
    expect(lockApi.unlock).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(screen.getByRole("button", { name: PAGE_UNLOCKED_LABEL })).toBeInTheDocument();
    expect(control.querySelector("svg.lucide-lock-open")).not.toBeNull();

    // The parent re-renders from the settled cache; the control holds the
    // transitional label until its own timer lapses.
    relock(false);
    expect(screen.getByRole("button", { name: PAGE_UNLOCKED_LABEL })).toBeInTheDocument();
    await settle(PAGE_UNLOCK_TRANSITION_MS);
    expect(screen.getByRole("button", { name: PAGE_LOCK_LABEL })).toBeInTheDocument();
  });

  it("rolls back to locked when the unlock is refused", async () => {
    lockApi.unlock.mockRejectedValue(new Error("denied"));
    const { relock } = renderControl(true);

    fireEvent.click(screen.getByRole("button", { name: PAGE_LOCKED_LABEL }));
    // The parent first paints the optimistic flip, then the rollback.
    relock(false);
    await settle();
    expect(notices.error).toHaveBeenCalled();
    relock(true);
    // The control follows the rolled-back row instead of reporting a
    // transitional state that never landed.
    expect(screen.getByRole("button", { name: PAGE_LOCKED_LABEL })).toBeInTheDocument();
    await settle(PAGE_UNLOCK_TRANSITION_MS);
    expect(screen.getByRole("button", { name: PAGE_LOCKED_LABEL })).toBeInTheDocument();
  });

  it("busies the control while the write flies", async () => {
    lockApi.lock.mockImplementation(() => new Promise<void>(() => undefined));
    renderControl(false);
    const control = screen.getByRole("button", { name: PAGE_LOCK_LABEL });
    fireEvent.click(control);
    await settle();
    expect(control).toHaveAttribute("aria-busy", "true");
    expect(control).toBeDisabled();
    fireEvent.click(control);
    expect(lockApi.lock).toHaveBeenCalledTimes(1);
  });

  it("replaces a pending transition when a newer unlock lands first", async () => {
    renderControl(true);
    fireEvent.click(screen.getByRole("button", { name: PAGE_LOCKED_LABEL }));
    await settle();
    expect(lockApi.unlock).toHaveBeenCalledTimes(1);

    await settle(PAGE_UNLOCK_TRANSITION_MS / 2);
    fireEvent.click(screen.getByRole("button", { name: PAGE_UNLOCKED_LABEL }));
    await settle();
    expect(lockApi.unlock).toHaveBeenCalledTimes(2);
    // The first timer was cleared: halfway through the second window the
    // control still reports the transitional phase.
    await settle(PAGE_UNLOCK_TRANSITION_MS / 2);
    expect(screen.getByRole("button", { name: PAGE_UNLOCKED_LABEL })).toBeInTheDocument();
    await settle(PAGE_UNLOCK_TRANSITION_MS / 2);
    expect(screen.queryByRole("button", { name: PAGE_UNLOCKED_LABEL })).not.toBeInTheDocument();
  });

  it("drops a pending transition on unmount", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const view = render(viewFor(client, true));
    fireEvent.click(screen.getByRole("button", { name: PAGE_LOCKED_LABEL }));
    await settle();
    expect(lockApi.unlock).toHaveBeenCalledTimes(1);
    view.unmount();
    await settle(PAGE_UNLOCK_TRANSITION_MS * 2);
  });
});
