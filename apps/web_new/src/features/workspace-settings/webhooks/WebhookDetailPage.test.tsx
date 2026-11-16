// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail (parity/specs/workspace-settings.md §21-§24,
// WSET-022/023/024): the read states, the prefilled form (URL, event mode
// derived from the flags, active switch), the Updating state and the
// notices, the one-time regenerated key with its fresh backup, and the
// danger zone's confirmation dialog.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  DETAIL_HEADING,
  DELETE_DIALOG_TITLE,
  REGENERATE_LABEL,
  UPDATING_LABEL,
  UPDATE_LABEL,
  WEBHOOK_DELETED_NOTICE,
  WEBHOOK_DELETE_FAILED_NOTICE,
  WEBHOOK_REGENERATED_NOTICE,
  WEBHOOK_REGENERATE_FAILED_BODY,
  WEBHOOK_REGENERATE_FAILED_NOTICE,
  WEBHOOK_UPDATED_NOTICE,
  WEBHOOK_UPDATE_FAILED_BODY,
  WEBHOOK_UPDATE_FAILED_NOTICE,
  WebhookDetailPage,
} from "./WebhookDetailPage.js";
import { webhookKeys } from "./api.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => navigate };
});

const download = vi.fn();
vi.mock("./backup.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./backup.js")>();
  return { ...original, downloadWebhookBackup: (...args: unknown[]) => download(...args) };
});

const { notifySuccess, notifyError } = await import("../../../shared/shell/index.js");

const SLUG = "acme";
const ID = "223e4567-e89b-12d3-a456-426614174001";

function row(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    project: true,
    issue: false,
    cycle: false,
    module: false,
    issue_comment: false,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

const EXISTING = "223e4567-e89b-12d3-a456-426614174009";

function renderPage(
  handler: (call: Call) => { status: number; body?: unknown } | "hold",
  options: { seedList?: boolean } = {}
) {
  const calls: Call[] = [];
  const transport: Transport = async (url, init) => {
    const call: Call = {
      method: init?.method ?? "GET",
      url,
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
    };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const answer = handler(call);
    if (answer === "hold") return new Promise(() => undefined);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedList === true) {
    queryClient.setQueryData(webhookKeys.list(SLUG), [row({ id: EXISTING, url: "https://example.com/other" })]);
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WebhookDetailPage workspaceSlug={SLUG} webhookId={ID} />
    </QueryClientProvider>
  );
  return { ...view, calls, queryClient };
}

afterEach(() => {
  cleanup();
  resetAppClient();
  vi.clearAllMocks();
});

describe("WebhookDetailPage", () => {
  it("shows a loader while the read is held", () => {
    renderPage(() => "hold");
    expect(screen.getByRole("status", { name: "Loading webhook details" })).toBeInTheDocument();
    expect(screen.queryByText(DETAIL_HEADING)).not.toBeInTheDocument();
  });

  it("explains a missing webhook instead of spinning (NEWFRONT-417)", async () => {
    renderPage(() => ({ status: 404 }));
    expect(await screen.findByTestId("workspace-settings-webhook-missing")).toBeInTheDocument();
  });

  it("renders a retryable failure", async () => {
    renderPage(() => ({ status: 500 }));
    expect(
      await screen.findByTestId("workspace-settings-webhook-detail-error", {}, { timeout: 5000 })
    ).toBeInTheDocument();
  });

  it("prefills the address, the individual mode and the active switch", async () => {
    renderPage(() => ({ status: 200, body: row() }));
    expect(await screen.findByText(DETAIL_HEADING)).toBeInTheDocument();
    expect(screen.getByLabelText("Target URL")).toHaveValue("https://example.com/post");
    expect(screen.getByRole("radio", { name: "Select individual events" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Projects" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Cycles" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Active" })).toBeChecked();
    expect(screen.getByText("Secret key")).toBeInTheDocument();
  });

  it("derives all-events mode when every flag is stored on", async () => {
    renderPage(() => ({
      status: 200,
      body: row({ project: true, issue: true, cycle: true, module: true, issue_comment: true }),
    }));
    await screen.findByText(DETAIL_HEADING);
    expect(screen.getByRole("radio", { name: "Send me everything" })).toBeChecked();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("blocks an empty address inline", async () => {
    const user = userEvent.setup();
    const { calls } = renderPage(() => ({ status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.clear(screen.getByLabelText("Target URL"));
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    expect(await screen.findByText("URL is required")).toBeInTheDocument();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("clears the inline address error as soon as a target URL is typed", async () => {
    const user = userEvent.setup();
    renderPage(() => ({ status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.clear(screen.getByLabelText("Target URL"));
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    expect(await screen.findByText("URL is required")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/typed");
    expect(screen.queryByText("URL is required")).not.toBeInTheDocument();
  });

  it("saves the URL, active state and flags with the updating state", async () => {
    const user = userEvent.setup();
    const held = Promise.resolve();
    const { calls } = renderPage((call) => {
      if (call.method === "GET") return { status: 200, body: row() };
      return { status: 200, body: row({ url: "https://example.com/next", is_active: false, module: true }) };
    });
    await screen.findByText(DETAIL_HEADING);
    await user.clear(screen.getByLabelText("Target URL"));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/next");
    await user.click(screen.getByRole("checkbox", { name: "Modules" }));
    await user.click(screen.getByRole("switch", { name: "Active" }));
    const pending = user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await held;
    await pending;
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_UPDATED_NOTICE));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      url: "https://example.com/next",
      is_active: false,
      project: true,
      cycle: false,
      module: true,
      issue: false,
      issue_comment: false,
    });
  });

  it("keeps an uncached list uncached after a save and a toggle", async () => {
    const user = userEvent.setup();
    const { queryClient } = renderPage((call) =>
      call.method === "GET" ? { status: 200, body: row() } : { status: 200, body: row({ is_active: false }) }
    );
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_UPDATED_NOTICE));
    await waitFor(() => expect(queryClient.getQueryState(webhookKeys.list(SLUG))).toBeUndefined());
    expect(queryClient.getQueryData(webhookKeys.detail(SLUG, ID))).toEqual(row({ is_active: false }));
  });

  it("updates the row in place when the list is already cached", async () => {
    const user = userEvent.setup();
    const { queryClient } = renderPage(
      (call) =>
        call.method === "GET" ? { status: 200, body: row() } : { status: 200, body: row({ is_active: false }) },
      { seedList: true }
    );
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await waitFor(() =>
      expect(
        (queryClient.getQueryData<Array<{ id: string; is_active: boolean }>>(webhookKeys.list(SLUG)) ?? []).map(
          (entry) => [entry.id, entry.is_active]
        )
      ).toEqual([
        [ID, false],
        [EXISTING, true],
      ])
    );
  });

  it("keeps an uncached list uncached after a rotation", async () => {
    const user = userEvent.setup();
    const rotated = row({ secret_key: "whsec_fixture_rotated_1234567890abcdef" });
    const { queryClient } = renderPage((call) =>
      call.method === "POST" ? { status: 200, body: rotated } : { status: 200, body: row() }
    );
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_REGENERATED_NOTICE));
    expect(queryClient.getQueryState(webhookKeys.list(SLUG))).toBeUndefined();
  });

  it("leaves no issued key in the mutation cache and forgets it on the next save", async () => {
    const user = userEvent.setup();
    const rotated = row({ secret_key: "whsec_fixture_rotated_1234567890abcdef" });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: Call[] = [];
    const transport: Transport = async (url, init) => {
      const call: Call = {
        method: init?.method ?? "GET",
        url,
        body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
      };
      calls.push(call);
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      const body = call.method === "POST" ? rotated : row();
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
    };
    createAppClient("https://api.test", transport);
    render(
      <QueryClientProvider client={queryClient}>
        <WebhookDetailPage workspaceSlug={SLUG} webhookId={ID} />
      </QueryClientProvider>
    );
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    expect(await screen.findByRole("button", { name: "Show secret key" })).toBeInTheDocument();
    const cached = queryClient
      .getMutationCache()
      .getAll()
      .map((mutation) => JSON.stringify(mutation.state.data ?? null));
    expect(cached.some((entry) => entry.includes("whsec_fixture_rotated"))).toBe(false);
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_UPDATED_NOTICE));
    expect(screen.queryByRole("button", { name: "Show secret key" })).not.toBeInTheDocument();
  });

  it("names the save and rotation failures in their notices", async () => {
    const user = userEvent.setup();
    renderPage((call) => (call.method === "GET" ? { status: 200, body: row() } : { status: 409 }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(WEBHOOK_UPDATE_FAILED_NOTICE, WEBHOOK_UPDATE_FAILED_BODY)
    );
    expect(WEBHOOK_UPDATE_FAILED_NOTICE).toBe("Webhook not updated");
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(WEBHOOK_REGENERATE_FAILED_NOTICE, WEBHOOK_REGENERATE_FAILED_BODY)
    );
    expect(WEBHOOK_REGENERATE_FAILED_NOTICE).toBe("Secret key not regenerated");
  });

  it("shows the Updating label while the save is in flight", async () => {
    const user = userEvent.setup();
    renderPage((call) => {
      if (call.method === "GET") return { status: 200, body: row() };
      return "hold";
    });
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    expect(await screen.findByRole("button", { name: UPDATING_LABEL })).toHaveAttribute("aria-busy", "true");
  });

  it("keeps the form and reports a failed save", async () => {
    const user = userEvent.setup();
    renderPage((call) => (call.method === "GET" ? { status: 200, body: row() } : { status: 409 }));
    await screen.findByText(DETAIL_HEADING);
    await user.clear(screen.getByLabelText("Target URL"));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/dup");
    await user.click(screen.getByRole("button", { name: UPDATE_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(WEBHOOK_UPDATE_FAILED_NOTICE, expect.any(String)));
    expect(screen.getByLabelText("Target URL")).toHaveValue("https://example.com/dup");
  });

  it("regenerates once, downloads a fresh backup and drops the old key", async () => {
    const user = userEvent.setup();
    const rotated = row({ secret_key: "whsec_fixture_rotated_1234567890abcdef" });
    const { calls } = renderPage((call) =>
      call.method === "POST" ? { status: 200, body: rotated } : { status: 200, body: row() }
    );
    await screen.findByText(DETAIL_HEADING);
    expect(screen.queryByRole("button", { name: "Show secret key" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_REGENERATED_NOTICE));
    expect(calls.at(-1)).toMatchObject({
      method: "POST",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/${ID}/regenerate/`,
    });
    expect(download).toHaveBeenCalledWith(
      expect.objectContaining({ content: expect.stringContaining("whsec_fixture_rotated_1234567890abcdef") })
    );
    expect(screen.getByRole("button", { name: "Show secret key" })).toBeInTheDocument();
  });

  it("shows the Re-generating label while rotating", async () => {
    const user = userEvent.setup();
    renderPage((call) => (call.method === "GET" ? { status: 200, body: row() } : "hold"));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    expect(await screen.findByRole("button", { name: "Re-generating…" })).toBeInTheDocument();
  });

  it("reports a failed regenerate without issuing a key", async () => {
    const user = userEvent.setup();
    renderPage((call) => (call.method === "GET" ? { status: 200, body: row() } : { status: 404 }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: REGENERATE_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(download).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Show secret key" })).not.toBeInTheDocument();
  });

  it("keeps the danger zone collapsed until expanded", async () => {
    const user = userEvent.setup();
    renderPage(() => ({ status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    expect(screen.getByText("Danger zone")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete webhook" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    expect(screen.getByRole("button", { name: "Delete webhook" })).toBeInTheDocument();
    expect(screen.getByText(/Once a webhook is deleted/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    expect(screen.queryByRole("button", { name: "Delete webhook" })).not.toBeInTheDocument();
  });

  it("cancels the delete dialog and keeps the webhook", async () => {
    const user = userEvent.setup();
    const { calls } = renderPage(() => ({ status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    await user.click(screen.getByRole("button", { name: "Delete webhook" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(DELETE_DIALOG_TITLE)).toBeInTheDocument();
    expect(within(dialog).getByText(/cannot be undone/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("closes the delete dialog on Escape", async () => {
    const user = userEvent.setup();
    renderPage(() => ({ status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    await user.click(screen.getByRole("button", { name: "Delete webhook" }));
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("deletes, announces and returns to the list", async () => {
    const user = userEvent.setup();
    const { calls } = renderPage((call) => (call.method === "DELETE" ? { status: 204 } : { status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    await user.click(screen.getByRole("button", { name: "Delete webhook" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_DELETED_NOTICE));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith(expect.objectContaining({ to: "/$ws/settings/webhooks" }))
    );
  });

  it("keeps the form when the delete fails", async () => {
    const user = userEvent.setup();
    renderPage((call) => (call.method === "DELETE" ? { status: 404 } : { status: 200, body: row() }));
    await screen.findByText(DETAIL_HEADING);
    await user.click(screen.getByRole("button", { name: "Danger zone" }));
    await user.click(screen.getByRole("button", { name: "Delete webhook" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(WEBHOOK_DELETE_FAILED_NOTICE, expect.any(String)));
    expect(screen.getByLabelText("Target URL")).toHaveValue("https://example.com/post");
    expect(navigate).not.toHaveBeenCalled();
  });
});
