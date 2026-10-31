// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-a-webhook dialog (parity/specs/workspace-settings.md §19/§20,
// WSET-020/021): the required URL with its inline error, the event modes,
// Cancel/Escape closing without a write, success landing on the reveal
// with its backup download and notice, and a failed create keeping the
// typed address.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CREATE_WEBHOOK_TITLE, CreateWebhookDialog, KEY_CREATED_TITLE } from "./CreateWebhookDialog.js";
import { WEBHOOK_CREATED_NOTICE, WEBHOOK_NOT_CREATED_NOTICE } from "./CreateWebhookDialog.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

const download = vi.fn();
vi.mock("./backup.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./backup.js")>();
  return { ...original, downloadWebhookBackup: (...args: unknown[]) => download(...args) };
});

const { notifySuccess, notifyError } = await import("../../../shared/shell/index.js");

const SLUG = "acme";
const ID = "223e4567-e89b-12d3-a456-426614174001";

function issued(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    project: true,
    issue: true,
    cycle: true,
    module: true,
    issue_comment: true,
    secret_key: "whsec_fixture_abcdefghijklmnopqrstuvwxyz",
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function renderDialog(
  handler: (call: Call) => { status: number; body?: unknown },
  whenCreate?: (call: Call) => Promise<{ status: number; body?: unknown }>
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
    const answer = whenCreate !== undefined && call.method === "POST" ? await whenCreate(call) : handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onOpenChange = vi.fn();
  const onCreated = vi.fn();
  function Harness(): React.ReactElement {
    const [open, setOpen] = React.useState(true);
    return (
      <CreateWebhookDialog
        workspaceSlug={SLUG}
        open={open}
        onCreated={onCreated}
        onOpenChange={(next) => {
          onOpenChange(next);
          setOpen(next);
        }}
      />
    );
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <Harness />
    </QueryClientProvider>
  );
  return { ...view, calls, onOpenChange, onCreated, queryClient };
}

afterEach(() => {
  cleanup();
  resetAppClient();
  vi.clearAllMocks();
});

describe("CreateWebhookDialog", () => {
  it("opens on the form with all events and no individual toggles", () => {
    renderDialog(() => ({ status: 500 }));
    expect(screen.getByText(CREATE_WEBHOOK_TITLE)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Send me everything" })).toBeChecked();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("blocks an empty URL inline without a request", async () => {
    const user = userEvent.setup();
    const { calls } = renderDialog(() => ({ status: 500 }));
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText("URL is required")).toBeInTheDocument();
    expect(calls.some((call) => call.method === "POST")).toBe(false);
    expect(screen.getByText(CREATE_WEBHOOK_TITLE)).toBeInTheDocument();
  });

  it("clears the inline error once the address changes", async () => {
    const user = userEvent.setup();
    renderDialog(() => ({ status: 500 }));
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText("URL is required")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Target URL"), "h");
    expect(screen.queryByText("URL is required")).not.toBeInTheDocument();
  });

  it("cancels without a write", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = renderDialog(() => ({ status: 500 }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("closes on Escape without a write", async () => {
    const user = userEvent.setup();
    const { calls, onOpenChange } = renderDialog(() => ({ status: 500 }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("sends the default all-on flags and lands on the reveal with the backup", async () => {
    const user = userEvent.setup();
    const { calls, onCreated } = renderDialog((call) =>
      call.method === "POST" ? { status: 201, body: issued() } : { status: 500 }
    );
    await user.type(screen.getByLabelText("Target URL"), "  https://example.com/post  ");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText(KEY_CREATED_TITLE)).toBeInTheDocument();
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      url: "https://example.com/post",
      project: true,
      cycle: true,
      module: true,
      issue: true,
      issue_comment: true,
    });
    expect(notifySuccess).toHaveBeenCalledWith(WEBHOOK_CREATED_NOTICE);
    expect(download).toHaveBeenCalledWith(
      expect.objectContaining({ fileName: expect.stringMatching(/^webhook-secret-key-\d+\.csv$/) })
    );
    expect(onCreated).toHaveBeenCalledWith(issued());
    expect(screen.getByText(/cannot be shown again/)).toBeInTheDocument();
  });

  it("sends the individual picks", async () => {
    const user = userEvent.setup();
    const { calls } = renderDialog((call) => ({ status: call.method === "POST" ? 201 : 500, body: issued() }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("radio", { name: "Select individual events" }));
    await user.click(screen.getByRole("checkbox", { name: "Cycles" }));
    await user.click(screen.getByRole("checkbox", { name: "Work item comments" }));
    await user.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByText(KEY_CREATED_TITLE);
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      url: "https://example.com/post",
      project: true,
      cycle: false,
      module: true,
      issue: true,
      issue_comment: false,
    });
  });

  it("keeps the dialog and the typed address when the create fails", async () => {
    const user = userEvent.setup();
    renderDialog(() => ({ status: 409 }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/dup");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(WEBHOOK_NOT_CREATED_NOTICE, expect.any(String)));
    expect(screen.getByText(CREATE_WEBHOOK_TITLE)).toBeInTheDocument();
    expect(screen.getByLabelText("Target URL")).toHaveValue("https://example.com/dup");
    expect(screen.queryByText(KEY_CREATED_TITLE)).not.toBeInTheDocument();
  });

  it("does not dismiss the reveal on Escape and only closes explicitly", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog(() => ({ status: 201, body: issued() }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByText(KEY_CREATED_TITLE);
    await user.keyboard("{Escape}");
    expect(screen.getByText(KEY_CREATED_TITLE)).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("leaves no key in the mutation cache after creation or close", async () => {
    const user = userEvent.setup();
    const { queryClient } = renderDialog(() => ({ status: 201, body: issued() }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByText(KEY_CREATED_TITLE);
    const cached = () =>
      JSON.stringify(
        queryClient
          .getMutationCache()
          .getAll()
          .map((mutation) => mutation.state.data)
      );
    expect(cached()).not.toContain("whsec_fixture_abcdefghijklmnopqrstuvwxyz");
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByText(KEY_CREATED_TITLE)).not.toBeInTheDocument();
    expect(cached()).not.toContain("whsec_fixture_abcdefghijklmnopqrstuvwxyz");
  });

  it("ignores a create answer that lands after Cancel and never reveals the key", async () => {
    const user = userEvent.setup();
    let release = (): void => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { calls, onCreated, queryClient } = renderDialog(
      () => ({ status: 500 }),
      async () => {
        await held;
        return { status: 201, body: issued() };
      }
    );
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(calls.find((call) => call.method === "POST")).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText(CREATE_WEBHOOK_TITLE)).not.toBeInTheDocument();
    release();
    await waitFor(() =>
      expect(
        queryClient.getQueryCache().findAll({ queryKey: ["acme", "workspace-settings", "webhooks", "list"] })
      ).toBeDefined()
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText(KEY_CREATED_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByText(/whsec_fixture/)).not.toBeInTheDocument();
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(download).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("ignores a failed create that lands after Escape", async () => {
    const user = userEvent.setup();
    let reject = (): void => undefined;
    const held = new Promise<void>((resolve) => {
      reject = resolve;
    });
    renderDialog(
      () => ({ status: 500 }),
      async () => {
        await held;
        return { status: 409 };
      }
    );
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/dup");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await user.keyboard("{Escape}");
    reject();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("resets the form, the error and the key when reopened", async () => {
    const user = userEvent.setup();
    const { rerender } = renderDialog((call) => ({ status: call.method === "POST" ? 201 : 500, body: issued() }));
    await user.type(screen.getByLabelText("Target URL"), "https://example.com/post");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByText(KEY_CREATED_TITLE);
    await user.click(screen.getByRole("button", { name: "Close" }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    rerender(
      <QueryClientProvider client={queryClient}>
        <CreateWebhookDialog workspaceSlug={SLUG} open={false} onOpenChange={() => undefined} />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <CreateWebhookDialog workspaceSlug={SLUG} open onOpenChange={() => undefined} />
      </QueryClientProvider>
    );
    expect(await screen.findByText(CREATE_WEBHOOK_TITLE)).toBeInTheDocument();
    expect(screen.getByLabelText("Target URL")).toHaveValue("");
  });
});
