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
import { ARCHIVED_TITLE, RESTORED_TITLE } from "../api/mutations.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { ProjectArchiveDialog, selectArchiveBody, type ArchiveDialogMode } from "./ProjectArchiveDialog.js";

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
    if (url.includes("/archive/")) {
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status >= 200 && status < 300 ? JSON.stringify({ archived_at: "2026-10-14T00:00:00Z" }) : "{}",
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function setup(mode: ArchiveDialogMode, status = 200) {
  const calls = stubClient(status);
  const onOpenChange = vi.fn();
  const onNavigate = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ProjectArchiveDialog
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Website relaunch"
        mode={mode}
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

describe("selectArchiveBody", () => {
  it("warns that archiving hides the project, its work items, cycles, modules and pages from lists and search", () => {
    const body = selectArchiveBody("archive", "Website relaunch");
    expect(body).toContain("Website relaunch");
    expect(body).toContain("archived");
    expect(body).toContain("projects list");
    expect(body).toContain("work items");
    expect(body).toContain("cycles");
    expect(body).toContain("modules");
    expect(body).toContain("pages");
    expect(body).toContain("search");
    expect(body).toContain("project admin restores");
  });

  it("explains that restoring returns the project", () => {
    expect(selectArchiveBody("restore", "Website relaunch")).toContain("Restoring a project");
  });
});

describe("ProjectArchiveDialog", () => {
  it("archives through the confirm control, then closes and returns to the list", async () => {
    const { calls, onOpenChange, onNavigate } = setup("archive");
    expect(screen.getByRole("heading", { name: "Archive Website relaunch" })).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(ARCHIVED_TITLE, "Website relaunch has been archived successfully")
    );
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/archive/"))).toBe(true);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onNavigate).toHaveBeenCalledWith("/acme/projects");
  });

  it("restores through the confirm control, then closes and returns to the list", async () => {
    const { calls, onOpenChange, onNavigate } = setup("restore");
    expect(screen.getByRole("heading", { name: "Restore Website relaunch" })).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(RESTORED_TITLE, "You can find Website relaunch in your projects.")
    );
    expect(calls.some((call) => call.startsWith("DELETE") && call.includes("/archive/"))).toBe(true);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onNavigate).toHaveBeenCalledWith("/acme/projects");
  });

  it("shows the working label while the write flies", async () => {
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
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ archived_at: "2026-10-14T00:00:00Z" }),
      };
    }) as never);
    const onOpenChange = vi.fn();
    const onNavigate = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ProjectArchiveDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Website relaunch"
          mode="archive"
          open
          onOpenChange={onOpenChange}
          onNavigate={onNavigate}
        />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Archiving…" })).not.toBeNull());
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
  });

  it("shows the restoring label while a restore flies", async () => {
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
        <ProjectArchiveDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Website relaunch"
          mode="restore"
          open
          onOpenChange={vi.fn()}
          onNavigate={vi.fn()}
        />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Restoring…" })).not.toBeNull());
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
  });

  it("keeps the dialog open when the write is refused", async () => {
    const { onOpenChange, onNavigate } = setup("archive", 400);

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("cancels without writing", () => {
    const { calls, onOpenChange, onNavigate } = setup("restore");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls).toHaveLength(0);
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onNavigate).not.toHaveBeenCalled();
  });
});
