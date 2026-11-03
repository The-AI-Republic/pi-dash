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

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DELETED_TITLE } from "../api/mutations.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { canDeleteProject, DELETE_CONFIRM_PHRASE, DeleteProjectDialog } from "./DeleteProjectDialog.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(status: number, calls: string[] = []) {
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
    return { status, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function setup(status = 204) {
  const calls = stubClient(status);
  const onOpenChange = vi.fn();
  const onNavigate = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DeleteProjectDialog
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Doomed Project"
        open
        onOpenChange={onOpenChange}
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { calls, onOpenChange, onNavigate };
}

function fillBoth(name: string, phrase: string) {
  fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: name } });
  fireEvent.change(screen.getByPlaceholderText(`Enter '${DELETE_CONFIRM_PHRASE}'`), { target: { value: phrase } });
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("canDeleteProject", () => {
  it("arms only on an exact name plus the exact phrase", () => {
    expect(canDeleteProject("Doomed Project", DELETE_CONFIRM_PHRASE, "Doomed Project")).toBe(true);
    expect(canDeleteProject("Wrong Name", DELETE_CONFIRM_PHRASE, "Doomed Project")).toBe(false);
    expect(canDeleteProject("Doomed Project", "wrong phrase", "Doomed Project")).toBe(false);
    expect(canDeleteProject("", "", "Doomed Project")).toBe(false);
  });
});

describe("DeleteProjectDialog", () => {
  it("warns that deletion is permanent", () => {
    setup();
    expect(screen.getByRole("heading", { name: "Delete project" })).not.toBeNull();
    expect(screen.getByText(/cannot be undone/)).not.toBeNull();
  });

  it("keeps the destructive button disabled until both fields are correct", () => {
    setup();
    const submit = screen.getByRole("button", { name: "Delete project" });
    expect(submit.hasAttribute("disabled")).toBe(true);

    fillBoth("Wrong Name", DELETE_CONFIRM_PHRASE);
    expect(screen.getByRole("button", { name: "Delete project" }).hasAttribute("disabled")).toBe(true);

    fillBoth("Doomed Project", "wrong phrase");
    expect(screen.getByRole("button", { name: "Delete project" }).hasAttribute("disabled")).toBe(true);

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    expect(screen.getByRole("button", { name: "Delete project" }).hasAttribute("disabled")).toBe(false);
  });

  it("ignores a click while the confirmations are incomplete", () => {
    const { calls } = setup();
    const submit = screen.getByRole("button", { name: "Delete project" });
    // Disabled controls swallow clicks; drop the attribute to prove the
    // handler's own gate also refuses an unconfirmed submission.
    (submit as HTMLButtonElement).disabled = false;
    fireEvent.click(submit);
    expect(calls).toHaveLength(0);
  });

  it("deletes on correct entries with a success notice and closes", async () => {
    const { calls, onOpenChange } = setup();

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(DELETED_TITLE, "Project deleted successfully."));
    expect(
      calls.some(
        (call) => call.startsWith("DELETE") && call.includes(`/projects/${PROJECT_ID}/`) && !call.includes("archive")
      )
    ).toBe(true);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("returns to the project list when the host provides navigation", async () => {
    const { onNavigate } = setup();
    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    await waitFor(() => expect(onNavigate).toHaveBeenCalledWith("/acme/projects"));
  });

  it("shows the working label while the delete flies", async () => {
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
      return { status: 204, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <DeleteProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Doomed Project"
          open
          onOpenChange={vi.fn()}
        />
      </QueryClientProvider>
    );

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Deleting…" })).not.toBeNull());
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
  });

  it("keeps the dialog open when the write is refused", async () => {
    const { onOpenChange } = setup(400);

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("cancels without writing", () => {
    const { calls, onOpenChange } = setup();

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls).toHaveLength(0);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("ignores a submit while disarmed", () => {
    const { calls } = setup();

    // fireEvent bypasses the disabled gate, so this pins the guard itself.
    fireEvent.click(screen.getByRole("button", { name: "Delete project" }));
    expect(calls).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("starts each visit with blank confirmations", () => {
    const onOpenChange = vi.fn();
    stubClient(204);
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <DeleteProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Doomed Project"
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );

    fillBoth("Doomed Project", DELETE_CONFIRM_PHRASE);
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <DeleteProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Doomed Project"
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <DeleteProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Doomed Project"
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    expect((screen.getByPlaceholderText("Project name") as HTMLInputElement).value).toBe("");
  });
});
