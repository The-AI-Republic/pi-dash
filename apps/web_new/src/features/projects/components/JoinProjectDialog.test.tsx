// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { JOIN_PROJECT_FAILED_TITLE } from "../../auth/index.js";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { JoinProjectDialog } from "./JoinProjectDialog.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(joinStatus: number, calls: string[] = []) {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
    calls.push(`${init?.method ?? "GET"} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/projects/invitations/")) {
      return {
        status: joinStatus,
        url,
        headers: { get: () => null },
        text: async () =>
          joinStatus === 201 ? JSON.stringify({ message: "joined" }) : JSON.stringify({ detail: "join refused" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function setup(joinStatus = 201, onNavigate: (href: string) => void = vi.fn()) {
  const calls = stubClient(joinStatus);
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <JoinProjectDialog
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Website relaunch"
        open
        onOpenChange={onOpenChange}
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { calls, onOpenChange, onNavigate };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("JoinProjectDialog", () => {
  it("names the project being joined", () => {
    setup();
    expect(screen.getByRole("heading", { name: "Join Project?" })).not.toBeNull();
    expect(screen.getByText(/Website relaunch/)).not.toBeNull();
  });

  it("joins, closes and lands inside the project", async () => {
    const onNavigate = vi.fn();
    const { calls, onOpenChange } = setup(201, onNavigate);

    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/issues`);
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/projects/invitations/"))).toBe(true);
  });

  it("shows the joining state while the request flies", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      calls.push(`${init?.method ?? "GET"} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      await gate;
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ message: "joined" }),
      };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <JoinProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Website relaunch"
          open
          onOpenChange={() => undefined}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    const joining = await screen.findByRole("button", { name: "Joining…" });
    expect(joining).toBeDisabled();
    release();
    await waitFor(() => expect(screen.getByRole("button", { name: "Join Project" })).not.toBeNull());
  });

  it("toasts the refusal and stays open when joining fails", async () => {
    const onNavigate = vi.fn();
    const { onOpenChange } = setup(400, onNavigate);

    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(JOIN_PROJECT_FAILED_TITLE, expect.any(String)));
    expect(screen.getByRole("heading", { name: "Join Project?" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("cancels without joining", () => {
    const { calls, onOpenChange } = setup();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.some((call) => call.includes("/projects/invitations/"))).toBe(false);
  });
});
