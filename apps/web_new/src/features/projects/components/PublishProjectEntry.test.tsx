// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PublishProjectEntry } from "./PublishProjectEntry.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const OPEN_TIMEOUT = { timeout: 10_000 };

afterEach(() => resetAppClient());

function renderEntry(open: boolean, onOpenChange = vi.fn()) {
  createAppClient("https://api.test", (async (url: string) => ({
    status: url.endsWith("/auth/get-csrf-token/") ? 200 : 200,
    url,
    headers: { get: () => null },
    text: async () =>
      url.endsWith("/auth/get-csrf-token/")
        ? JSON.stringify({ csrf_token: "csrf" })
        : JSON.stringify({
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
          }),
  })) as never);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PublishProjectEntry
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Web"
        open={open}
        onOpenChange={onOpenChange}
      />
    </QueryClientProvider>
  );
  return onOpenChange;
}

describe("PublishProjectEntry", () => {
  it("mounts nothing until the first open", () => {
    renderEntry(false);
    expect(screen.queryByRole("heading", { name: "Publish project" })).not.toBeInTheDocument();
  });

  it("loads and shows the dialog once opened", async () => {
    renderEntry(true);
    expect(await screen.findByRole("heading", { name: "Publish project" }, OPEN_TIMEOUT)).toBeInTheDocument();
    expect(screen.getByText("Share Web on a public board.")).toBeInTheDocument();
  });

  it("forwards close requests from the dialog", async () => {
    const onOpenChange = renderEntry(true);
    (await screen.findByRole("button", { name: "Close dialog" }, OPEN_TIMEOUT)).click();
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
