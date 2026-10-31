// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Publish dialog (parity/specs/project-settings-lifecycle.md §PSET-016):
// loader, layout floor, independent toggles, live link with open/copy,
// dirty-gated update and separate unpublish.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  PUBLISH_LAYOUT_REMOVE_REFUSED,
  PUBLISH_LAYOUT_REQUIRED,
  PUBLISH_LINK_COPY_FAILED_TITLE,
  PUBLISH_LINK_COPIED_TITLE,
  PublishProjectDialog,
} from "./PublishProjectDialog.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const BOARD_ID = "323e4567-e89b-12d3-a456-426614174002";

function board(overrides: Record<string, unknown> = {}) {
  return {
    id: BOARD_ID,
    project_details: {
      id: PROJECT_ID,
      identifier: "WEB",
      name: "Web",
      cover_image: null,
      cover_image_url: null,
      logo_props: {},
      description: "",
      is_default: false,
    },
    workspace_detail: { id: "123e4567-e89b-12d3-a456-426614174000", name: "Acme", slug: "acme", logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: PROJECT_ID,
    entity_identifier: null,
    entity_name: "project",
    anchor: "live-token",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    intake: null,
    is_votes_enabled: false,
    view_props: { list: true, kanban: true },
    is_activity_enabled: false,
    is_disabled: false,
    ...overrides,
  };
}

function emptyBoard() {
  return {
    deleted_at: null,
    entity_identifier: null,
    entity_name: "",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: null,
    is_activity_enabled: false,
    is_disabled: false,
    created_by: null,
    updated_by: null,
    intake: null,
  };
}

function stub(
  body: unknown = emptyBoard(),
  options: { gate?: Promise<void>; writeGate?: Promise<void>; status?: number; writeBody?: unknown } = {}
) {
  const calls: { method: string; url: string; body: string }[] = [];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    calls.push({ method, url, body: init?.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (method === "GET" && options.gate !== undefined) await options.gate;
    if (method !== "GET" && options.writeGate !== undefined) await options.writeGate;
    const status = method === "GET" ? 200 : (options.status ?? 200);
    const responseBody = method === "GET" ? body : (options.writeBody ?? board());
    return {
      status: method === "DELETE" && status < 400 ? 204 : status,
      url,
      headers: { get: () => null },
      text: async () =>
        method === "DELETE" && status < 400 ? "" : JSON.stringify(status >= 400 ? { detail: "refused" } : responseBody),
    };
  }) as never);
  return calls;
}

function setup(
  body: unknown = emptyBoard(),
  options: { gate?: Promise<void>; writeGate?: Promise<void>; status?: number; writeBody?: unknown } = {}
) {
  const calls = stub(body, options);
  const onOpenChange = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PublishProjectDialog
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Web"
        open
        onOpenChange={onOpenChange}
      />
    </QueryClientProvider>
  );
  return { calls, onOpenChange };
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("PublishProjectDialog", () => {
  it("shows a loader while settings load, then both layouts with toggles off", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    setup(emptyBoard(), { gate });
    expect(screen.getByTestId("publish-loading")).toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.queryByTestId("publish-loading")).toBeNull());
    expect(screen.getByLabelText("List")).toBeChecked();
    expect(screen.getByLabelText("Kanban")).toBeChecked();
    expect(screen.getByLabelText("Comments")).not.toBeChecked();
    expect(screen.getByLabelText("Reactions")).not.toBeChecked();
    expect(screen.getByLabelText("Voting")).not.toBeChecked();
  });

  it("refuses to remove the last layout", async () => {
    setup();
    await waitFor(() => expect(screen.getByLabelText("List")).toBeInTheDocument());
    fireEvent.click(screen.getByLabelText("Kanban"));
    fireEvent.click(screen.getByLabelText("List"));
    expect(screen.getByText(PUBLISH_LAYOUT_REMOVE_REFUSED)).toBeInTheDocument();
    expect(screen.getByLabelText("List")).toBeChecked();
  });

  it("refuses to submit a stored board that has no layout enabled", async () => {
    setup(board({ anchor: null, view_props: { list: false, kanban: false } }));
    await waitFor(() => expect(screen.getByLabelText("List")).not.toBeChecked());
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    expect(screen.getByText(PUBLISH_LAYOUT_REQUIRED)).toBeInTheDocument();
  });

  it("publishes with the chosen layouts and toggles", async () => {
    const { calls } = setup();
    await waitFor(() => expect(screen.getByLabelText("List")).toBeInTheDocument());
    fireEvent.click(screen.getByLabelText("Comments"));
    fireEvent.click(screen.getByLabelText("Voting"));
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    const post = calls.find((call) => call.method === "POST");
    expect(post).toBeDefined();
    expect(JSON.parse(post?.body ?? "{}")).toEqual({
      views: { list: true, kanban: true },
      is_comments_enabled: true,
      is_reactions_enabled: false,
      is_votes_enabled: true,
    });
  });

  it("shows the live link with open and copy actions once published", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(globalThis.navigator, "clipboard", { configurable: true, value: { writeText } });
    setup(board());
    const link = await screen.findByRole("link", { name: "Open published board" });
    expect(link).toHaveAttribute("target", "_blank");
    expect(link.getAttribute("href")).toContain("/issues/live-token");
    fireEvent.click(screen.getByRole("button", { name: "Copy published link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(expect.stringContaining("/issues/live-token")));
    expect(notifySuccess).toHaveBeenCalledWith(PUBLISH_LINK_COPIED_TITLE);
  });

  it("reports a refused clipboard write", async () => {
    Object.defineProperty(globalThis.navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async () => {
          throw new Error("denied");
        },
      },
    });
    setup(board());
    fireEvent.click(await screen.findByRole("button", { name: "Copy published link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
  });

  it("reports a window with no clipboard instead of silently doing nothing", async () => {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
    setup(board());
    fireEvent.click(await screen.findByRole("button", { name: "Copy published link" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        PUBLISH_LINK_COPY_FAILED_TITLE,
        "The clipboard is not available in this window."
      )
    );
  });

  it("hides update until dirty, then patches the board", async () => {
    const { calls, onOpenChange } = setup(board());
    await screen.findByRole("button", { name: "Unpublish" });
    expect(screen.queryByRole("button", { name: "Update" })).toBeNull();
    fireEvent.click(screen.getByLabelText("Voting"));
    fireEvent.click(screen.getByRole("button", { name: "Update" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(BOARD_ID);
  });

  it("reports a failed update without closing", async () => {
    const { onOpenChange } = setup(board(), { status: 500 });
    fireEvent.click(await screen.findByLabelText("Voting"));
    fireEvent.click(await screen.findByRole("button", { name: "Update" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("shows the publish progress label while a create is in flight", async () => {
    let release!: () => void;
    const writeGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    setup(emptyBoard(), { writeGate });
    await waitFor(() => expect(screen.getByLabelText("List")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    expect(await screen.findByRole("button", { name: "Publishing…" })).toBeDisabled();
    release();
  });

  it("unpublishes separately", async () => {
    const { calls } = setup(board());
    fireEvent.click(await screen.findByRole("button", { name: "Unpublish" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(BOARD_ID))).toBe(true);
  });

  it("reports a failed unpublish and keeps the board visible", async () => {
    setup(board(), { status: 500 });
    fireEvent.click(await screen.findByRole("button", { name: "Unpublish" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't unpublish the project", expect.any(String)));
    expect(screen.getByTestId("publish-live-link-row")).toBeInTheDocument();
  });

  it("shows the unpublish progress label while the delete is in flight", async () => {
    let release!: () => void;
    const writeGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    setup(board(), { writeGate });
    fireEvent.click(await screen.findByRole("button", { name: "Unpublish" }));
    expect(await screen.findByRole("button", { name: "Unpublishing…" })).toBeDisabled();
    release();
  });

  it("shows a failed publish through the toast", async () => {
    setup(emptyBoard(), { status: 400 });
    await waitFor(() => expect(screen.getByLabelText("List")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
  });

  it("renders nothing while closed", () => {
    stub();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PublishProjectDialog
          workspaceSlug="acme"
          projectId={PROJECT_ID}
          projectName="Web"
          open={false}
          onOpenChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.queryByRole("heading", { name: "Publish project" })).toBeNull();
  });

  it("closes through the dialog", async () => {
    const { onOpenChange } = setup();
    fireEvent.click(await screen.findByRole("button", { name: "Close dialog" }));
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
