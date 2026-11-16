// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail outlet (WSET-036, spec §36): a loading state while the
// address read runs, a missing-or-deleted notice for a 404, a retryable
// error for anything else, and the registered body once the slice lands.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { registerWorkspaceWebhookDetailPage, resetWorkspaceSettingsPages } from "../lib/registry.js";
import {
  SettingsWebhookDetailOutlet,
  WEBHOOK_DETAIL_MISSING_HEADING,
  WebhookDetailError,
} from "./SettingsWebhookDetailOutlet.js";

const HOOK = "223e4567-e89b-12d3-a456-426614174001";

/** Render the outlet against a stub transport answering the detail read. */
function renderOutlet(status: number | "pending") {
  const client = createAppClient("https://api.test", async (url) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    if (status === "pending") return new Promise(() => undefined);
    return { status, url, headers: { get: () => null }, text: async () => "{}" };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SettingsWebhookDetailOutlet workspaceSlug="acme" webhookId={HOOK} />
    </QueryClientProvider>
  );
  return { client, queryClient, ...view };
}

afterEach(() => {
  // Unmount first: a mounted tree must not re-render against a client
  // this teardown is about to drop.
  cleanup();
  resetWorkspaceSettingsPages();
  resetAppClient();
});

describe("SettingsWebhookDetailOutlet", () => {
  it("shows a loading state while the address read runs", () => {
    renderOutlet("pending");
    expect(screen.getByTestId("workspace-settings-webhook-loading")).toBeInTheDocument();
  });

  it("explains an unknown id as missing instead of spinning", async () => {
    renderOutlet(404);
    expect(await screen.findByRole("heading", { name: WEBHOOK_DETAIL_MISSING_HEADING })).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-webhook-loading")).not.toBeInTheDocument();
  });

  it("renders a retryable error for other failures", async () => {
    renderOutlet(500);
    expect(await screen.findByTestId("workspace-settings-webhook-error")).toBeInTheDocument();
  });

  it("renders the registered detail body for a known id", async () => {
    renderOutlet(200);
    await act(async () => {
      registerWorkspaceWebhookDetailPage(({ workspaceSlug, webhookId }) => (
        <p>
          {workspaceSlug} hook {webhookId}
        </p>
      ));
    });
    expect(screen.getByText(`acme hook ${HOOK}`)).toBeInTheDocument();
  });

  it("falls back to the quiet placeholder when the id resolves without a body", async () => {
    renderOutlet(200);
    expect(await screen.findByTestId("workspace-settings-section-placeholder")).toBeInTheDocument();
  });

  it("keeps the error surface free of controls", () => {
    render(<WebhookDetailError />);
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
