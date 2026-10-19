// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { LEAVE_CONFIRM_PHRASE, LeaveProjectDialog } from "./LeaveProjectDialog.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(leaveStatus: number) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    return {
      status: leaveStatus,
      url,
      headers: { get: () => null },
      text: async () => (leaveStatus === 204 ? "" : JSON.stringify({ detail: "The only admin cannot leave." })),
    };
  }) as never);
  return calls;
}

function setup(leaveStatus = 204, onNavigate: (href: string) => void = vi.fn()) {
  const calls = stubClient(leaveStatus);
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <LeaveProjectDialog
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Web"
        open
        onOpenChange={onOpenChange}
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { calls, onOpenChange, onNavigate };
}

/**
 * Type both confirmations after focusing each field: the dialog moves
 * focus asynchronously on open, and user-event retries otherwise land the
 * text in whichever field happened to hold focus.
 */
async function fillConfirmations(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
  phrase: string
): Promise<void> {
  const nameInput = screen.getByLabelText(/Enter the project name/);
  await user.click(nameInput);
  await user.type(nameInput, name);
  expect(nameInput).toHaveValue(name);
  const phraseInput = screen.getByLabelText(/To confirm/);
  await user.click(phraseInput);
  await user.type(phraseInput, phrase);
  expect(phraseInput).toHaveValue(phrase);
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("LeaveProjectDialog", () => {
  it("blocks a wrong project name with a corrective error", async () => {
    const user = userEvent.setup();
    const { calls } = setup();
    await fillConfirmations(user, "Wrong Name", LEAVE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    expect(screen.getByText(/exactly as shown/)).toBeInTheDocument();
    expect(calls.some((url) => url.includes("/members/leave/"))).toBe(false);
  });

  it("blocks a wrong confirmation phrase once the name matches", async () => {
    const user = userEvent.setup();
    const { calls } = setup();
    await fillConfirmations(user, "Web", "leave");
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    expect(screen.queryByText(/exactly as shown/)).toBeNull();
    expect(screen.getByText(/exactly to confirm leaving/)).toBeInTheDocument();
    expect(calls.some((url) => url.includes("/members/leave/"))).toBe(false);
  });

  it("leaves, toasts, closes and lands on the projects list", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    const { calls, onOpenChange } = setup(204, onNavigate);
    await fillConfirmations(user, "Web", LEAVE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Left the project", expect.any(String)));
    expect(calls.some((url) => url.endsWith(`/projects/${PROJECT_ID}/members/leave/`))).toBe(true);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onNavigate).toHaveBeenCalledWith("/acme/projects");
  });

  it("toasts the failure and stays open when leaving is refused", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    const { onOpenChange } = setup(400, onNavigate);
    await fillConfirmations(user, "Web", LEAVE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't leave the project", expect.any(String)));
    expect(screen.getByRole("heading", { name: "Leave project" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("clears the confirmations between visits", async () => {
    const user = userEvent.setup();
    stubClient(204);
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Web"
          open
          onOpenChange={onOpenChange}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );
    await fillConfirmations(user, "Wrong Name", LEAVE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    expect(screen.getByText(/exactly as shown/)).toBeInTheDocument();
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Web"
          open={false}
          onOpenChange={onOpenChange}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Web"
          open
          onOpenChange={onOpenChange}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByLabelText(/Enter the project name/)).toHaveValue("");
    expect(screen.getByLabelText(/To confirm/)).toHaveValue("");
    expect(screen.queryByText(/exactly as shown/)).toBeNull();
  });

  it("disables the leave button with a progress label while leaving", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      await gate;
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }) as never);
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Web"
          open
          onOpenChange={onOpenChange}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );
    await fillConfirmations(user, "Web", LEAVE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Leave project" }));
    const leaving = await screen.findByRole("button", { name: "Leaving…" });
    expect(leaving).toBeDisabled();
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Left the project", expect.any(String)));
  });

  it("cancels without leaving", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = setup();
    await user.type(screen.getByLabelText(/To confirm/), LEAVE_CONFIRM_PHRASE);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.some((url) => url.includes("/members/leave/"))).toBe(false);
  });
});
