// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { QuickLink } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { QuickLinkDialog } from "./QuickLinkDialog.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const SAVED: QuickLink = {
  id: "11111111-1111-4111-8111-111111111111",
  workspace: "22222222-2222-4222-8222-222222222222",
  owner: "33333333-3333-4333-8333-333333333333",
  title: "Team docs",
  url: "https://example.com/docs",
  metadata: {},
  created_by: null,
  updated_by: null,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

function stubTransport(status: number, calls: Array<{ url: string; method: string }>) {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
    calls.push({ url, method: init?.method ?? "GET" });
    return {
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => (status >= 200 && status < 300 ? JSON.stringify(SAVED) : JSON.stringify({})),
    };
  }) as never);
}

/**
 * Base UI mounts the dialog synchronously but moves initial focus into it in
 * a follow-up task. Typing before that task lands lets the focus move steal
 * keystrokes (the dialog's close button, or the popup itself, takes focus
 * mid-type), so tests must wait for the focus move before touching the form.
 */
async function waitForDialogFocus(): Promise<HTMLElement> {
  const dialog = screen.getByRole("dialog");
  await waitFor(() => expect(dialog).toContainElement(document.activeElement as HTMLElement));
  return dialog;
}

async function setup(link: QuickLink | null, status = 200) {
  const calls: Array<{ url: string; method: string }> = [];
  stubTransport(status, calls);
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <QuickLinkDialog workspaceSlug="acme" link={link} open onOpenChange={onOpenChange} />
    </QueryClientProvider>
  );
  await waitForDialogFocus();
  return { calls, onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("QuickLinkDialog", () => {
  it("saves a fresh link with a completed scheme and closes", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = await setup(null);
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();

    await user.type(screen.getByLabelText("Address"), "example.com/docs");
    await user.type(screen.getByLabelText("Name"), "Team docs");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/quick-links/"))).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith("Link saved");
  });

  it("saves an edit through PATCH and closes", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = await setup(SAVED);
    expect(screen.getByRole("dialog", { name: "Edit link" })).toBeInTheDocument();
    expect(screen.getByLabelText("Address")).toHaveValue("https://example.com/docs");
    expect(screen.getByLabelText("Name")).toHaveValue("Team docs");

    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "Handbook");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(calls.some((call) => call.method === "PATCH" && call.url.endsWith(`/quick-links/${SAVED.id}/`))).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith("Link updated");
  });

  it("blocks an empty address inline and keeps the dialog", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = await setup(null);
    await user.click(screen.getByRole("button", { name: "Save link" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Enter the link address.");
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    expect(calls.filter((call) => call.url.includes("/quick-links/"))).toEqual([]);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("blocks a malformed address inline", async () => {
    const user = userEvent.setup();
    await setup(null);
    await user.type(screen.getByLabelText("Address"), "not a url");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Enter a valid link address.");
  });

  it("clears the error while the address is edited", async () => {
    const user = userEvent.setup();
    await setup(null);
    await user.click(screen.getByRole("button", { name: "Save link" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Address"), "example.com");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("submits from the keyboard through the form", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = await setup(null);
    await user.type(screen.getByLabelText("Address"), "example.com/docs");
    fireEvent.submit(screen.getByLabelText("Address").closest("form") as HTMLFormElement);

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("shows progress and locks cancel while the write lands", async () => {
    const user = userEvent.setup();
    const calls: Array<{ url: string; method: string }> = [];
    let release: (value: unknown) => void = () => {};
    const gate = new Promise((resolve) => {
      release = resolve as (value: unknown) => void;
    });
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      calls.push({ url, method: init?.method ?? "GET" });
      await gate;
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(SAVED),
      };
    }) as never);
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <QuickLinkDialog workspaceSlug="acme" link={null} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    await waitForDialogFocus();

    await user.type(screen.getByLabelText("Address"), "example.com/docs");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Save link" })).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    // A second submit while pending is ignored.
    fireEvent.submit(screen.getByLabelText("Address").closest("form") as HTMLFormElement);

    release(undefined);
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(calls.filter((call) => call.method === "POST")).toHaveLength(1);
  });

  it("keeps the dialog open with a failure notice when the write fails", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = await setup(null, 500);
    await user.type(screen.getByLabelText("Address"), "example.com/docs");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Link save failed", expect.anything()));
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("cancels without storing", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = await setup(null);
    await user.type(screen.getByLabelText("Address"), "example.com/docs");
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.filter((call) => call.url.includes("/quick-links/"))).toEqual([]);
  });
});
