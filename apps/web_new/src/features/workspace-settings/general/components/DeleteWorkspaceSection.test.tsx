// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete danger zone (WSET-006): the section copy, the double confirmation
// gate, cancel/dismiss resetting without a write, the working state, the
// landing navigation on success and the kept dialog on failure.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { DeleteWorkspaceSection, DELETE_DIALOG_TITLE, DELETE_SECTION_TITLE } from "./DeleteWorkspaceSection.js";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";

const DETAILS_PATH = "/api/workspaces/acme/";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function stubApi(options: { deleteStatus?: number; hold?: Promise<void> } = {}) {
  const deletes: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(DETAILS_PATH) && method === "DELETE") {
      deletes.push(url);
      if (options.hold !== undefined) await options.hold;
      return jsonResponse(url, options.deleteStatus ?? 204, {});
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { deletes };
}

function renderSection(overrides: Partial<React.ComponentProps<typeof DeleteWorkspaceSection>> = {}) {
  const onNavigate = vi.fn();
  const props: React.ComponentProps<typeof DeleteWorkspaceSection> = {
    workspaceSlug: "acme",
    workspaceName: "Acme Name",
    destination: "/beta/",
    onNavigate,
    ...overrides,
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DeleteWorkspaceSection {...props} />
    </QueryClientProvider>
  );
  return { props, onNavigate };
}

async function openDialog(): Promise<void> {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Delete" }));
  await screen.findByLabelText(/Type the workspace name/);
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("DeleteWorkspaceSection", () => {
  it("names workspace deletion and explains the irreversibility", () => {
    stubApi();
    renderSection();
    expect(screen.getByRole("heading", { name: DELETE_SECTION_TITLE })).toBeInTheDocument();
    expect(screen.getByText(/permanently/)).toBeInTheDocument();
  });

  it("keeps Confirm disabled until the exact name and phrase match", async () => {
    stubApi();
    renderSection();
    await openDialog();
    const user = userEvent.setup();
    expect(screen.getByRole("heading", { name: DELETE_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toHaveTextContent("Acme Name");
    expect(screen.getByText(/delete my workspace/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();
    await user.type(screen.getByLabelText(/Type the workspace name/), "wrong name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();
    await user.clear(screen.getByLabelText(/Type the workspace name/));
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.clear(screen.getByLabelText(/For final confirmation/));
    await user.type(screen.getByLabelText(/For final confirmation/), "wrong phrase");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();
    await user.clear(screen.getByLabelText(/For final confirmation/));
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeEnabled();
  });

  it("cancelling resets the inputs without a write", async () => {
    const { deletes } = stubApi();
    renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByLabelText(/Type the workspace name/)).not.toBeInTheDocument();
    });
    expect(deletes).toHaveLength(0);
    await openDialog();
    expect(screen.getByLabelText(/Type the workspace name/)).toHaveValue("");
    expect(screen.getByLabelText(/For final confirmation/)).toHaveValue("");
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("dismissing with Escape resets the inputs without a write", async () => {
    const { deletes } = stubApi();
    renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByLabelText(/Type the workspace name/)).not.toBeInTheDocument();
    });
    expect(deletes).toHaveLength(0);
  });

  it("shows the working state, deletes and navigates away", async () => {
    let release: () => void = () => undefined;
    const hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { deletes } = stubApi({ hold });
    const { onNavigate } = renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByRole("button", { name: "Deleting" })).toBeInTheDocument();
    release();
    await waitFor(() => {
      expect(onNavigate).toHaveBeenCalledWith("/beta/");
    });
    expect(deletes).toEqual(["https://api.test/api/workspaces/acme/"]);
    expect(notifySuccess).toHaveBeenCalledWith("Workspace deleted.");
  });

  it("deletes from the dialog form's submit as well as the Confirm button", async () => {
    const { deletes } = stubApi();
    const { onNavigate } = renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    const form = screen.getByLabelText(/Type the workspace name/).closest("form");
    if (form === null) throw new Error("missing delete form");
    fireEvent.submit(form);
    await waitFor(() => {
      expect(onNavigate).toHaveBeenCalledWith("/beta/");
    });
    expect(deletes).toEqual(["https://api.test/api/workspaces/acme/"]);
    expect(notifySuccess).toHaveBeenCalledWith("Workspace deleted.");
  });

  it("ignores a confirm while the delete is already in flight", async () => {
    let release: () => void = () => undefined;
    const hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { deletes } = stubApi({ hold });
    renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    const form = screen.getByLabelText(/Type the workspace name/).closest("form");
    if (form === null) throw new Error("missing delete form");
    fireEvent.submit(form);
    fireEvent.submit(form);
    await waitFor(() => {
      expect(deletes).toHaveLength(1);
    });
    release();
    fireEvent.submit(form);
    expect(deletes).toHaveLength(1);
  });

  it("keeps the dialog and its input when the delete fails", async () => {
    const { deletes } = stubApi({ deleteStatus: 403 });
    const { onNavigate } = renderSection();
    await openDialog();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/Type the workspace name/), "Acme Name");
    await user.type(screen.getByLabelText(/For final confirmation/), "delete my workspace");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => {
      expect(notifyError).toHaveBeenCalledWith("That didn't work.", expect.any(String));
    });
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(onNavigate).not.toHaveBeenCalled();
    expect(screen.getByLabelText(/Type the workspace name/)).toHaveValue("Acme Name");
    expect(deletes).toHaveLength(1);
  });
});
