// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plan badge (parity/specs/cloud.md §8, CLOUD-038): the label per plan
// state, the account-center destination, the version hint, the focus
// refresh, the no-op refresh, and the last-known label after a failure.
import { QueryClient, QueryClientProvider, focusManager } from "@tanstack/react-query";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Transport } from "@pidash/api-client";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { PlanBadge, planBadgeHint } from "./PlanBadge.js";

type Reply = { status: number; body: unknown } | "throw";

function renderBadge(replies: Reply[]) {
  let index = 0;
  const transport: Transport = async (url) => {
    const reply = replies[Math.min(index, replies.length - 1)];
    index += 1;
    if (reply === "throw") throw new Error("offline");
    return {
      status: reply?.status ?? 500,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(reply?.body ?? {}),
    };
  };
  const client = createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true } },
  });
  let renders = 0;
  function Counted(): React.ReactElement {
    renders += 1;
    return <PlanBadge version="9.9.9" />;
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <Counted />
    </QueryClientProvider>
  );
  return { ...view, view, queryClient, client, calls: () => index, renders: () => renders };
}

function free(overrides: Record<string, unknown> = {}) {
  return { is_paid: false, migration_required: false, user_center_url: "https://home.test/account", ...overrides };
}

beforeEach(() => {
  edition.flags["plan-badge"] = true;
});

afterEach(() => {
  delete edition.flags["plan-badge"];
  cleanup();
  resetAppClient();
  vi.restoreAllMocks();
});

describe("PlanBadge", () => {
  it("labels a free account as an upgrade and opens the account center", async () => {
    const user = userEvent.setup();
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    renderBadge([{ status: 200, body: free() }]);
    const badge = await screen.findByRole("button", { name: "Plan: Upgrade" });
    expect(badge).toHaveTextContent("Upgrade");
    await user.click(badge);
    // The platform opener: the desktop build opens its own window.
    expect(openExternal).toHaveBeenCalledWith("https://home.test/account");
  });

  it("names a paid plan and asks a legacy plan to migrate", async () => {
    renderBadge([{ status: 200, body: free({ is_paid: true, plan_name: "Team" }) }]);
    expect(await screen.findByRole("button", { name: "Plan: Team" })).toBeInTheDocument();
    cleanup();
    renderBadge([{ status: 200, body: free({ migration_required: true }) }]);
    expect(await screen.findByRole("button", { name: "Plan: Migrate plan" })).toBeInTheDocument();
  });

  it("carries the running version in its hint", async () => {
    expect(planBadgeHint("1.2.3")).toBe("Version: v1.2.3");
    renderBadge([{ status: 200, body: free() }]);
    const badge = await screen.findByRole("button", { name: "Plan: Upgrade" });
    expect(badge).toHaveAttribute("title", "Version: v9.9.9");
  });

  it("renders nothing on an edition without the plan-badge flag", () => {
    delete edition.flags["plan-badge"];
    renderBadge([{ status: 200, body: free() }]);
    expect(screen.queryByTestId("cloud-plan-badge")).not.toBeInTheDocument();
    expect(screen.queryByText("Upgrade")).not.toBeInTheDocument();
  });

  it("renders nothing before the first answer", () => {
    renderBadge(["throw"]);
    expect(screen.queryByTestId("cloud-plan-badge")).not.toBeInTheDocument();
  });

  it("opens the account center in a new tab through the platform", async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    renderBadge([{ status: 200, body: free() }]);
    await user.click(await screen.findByRole("button", { name: "Plan: Upgrade" }));
    expect(open).toHaveBeenCalledWith("https://home.test/account", "_blank", "noopener");
  });

  it("stays inert when the plan carries no destination", async () => {
    const user = userEvent.setup();
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    renderBadge([{ status: 200, body: free({ user_center_url: "" }) }]);
    const badge = await screen.findByRole("button", { name: "Plan: Upgrade" });
    expect(badge).toHaveAttribute("aria-disabled", "true");
    await user.click(badge);
    expect(openExternal).not.toHaveBeenCalled();
  });

  /** Leave and re-enter the window the way the query client notices. */
  async function refocus(): Promise<void> {
    await act(async () => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
  }

  it("refreshes when the window regains focus and skips the render when nothing changed", async () => {
    const harness = renderBadge([{ status: 200, body: free() }]);
    await screen.findByRole("button", { name: "Plan: Upgrade" });
    expect(harness.calls()).toBe(1);
    const before = harness.renders();
    await refocus();
    await waitFor(() => expect(harness.calls()).toBe(2));
    expect(harness.renders()).toBe(before);
  });

  it("updates on focus when the plan changed", async () => {
    renderBadge([
      { status: 200, body: free() },
      { status: 200, body: free({ is_paid: true, plan_name: "Scale" }) },
    ]);
    await screen.findByRole("button", { name: "Plan: Upgrade" });
    await refocus();
    expect(await screen.findByRole("button", { name: "Plan: Scale" })).toBeInTheDocument();
  });

  it("keeps the last known label after a refresh failure and retries on the next focus", async () => {
    const harness = renderBadge([{ status: 200, body: free({ is_paid: true, plan_name: "Team" }) }, "throw"]);
    await screen.findByRole("button", { name: "Plan: Team" });
    await refocus();
    await waitFor(() => expect(harness.calls()).toBe(2));
    expect(screen.getByRole("button", { name: "Plan: Team" })).toBeInTheDocument();
    await refocus();
    await waitFor(() => expect(harness.calls()).toBe(3));
    expect(screen.getByRole("button", { name: "Plan: Team" })).toBeInTheDocument();
  });
});
