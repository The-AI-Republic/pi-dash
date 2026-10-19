// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { IssueDetailWidgets } from "./IssueDetailWidgets.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";

function renderWidgets() {
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (init.method === "GET" && url.endsWith(`/issues/${ISSUE}/attachments/`)) return json(200, []);
    if (init.method === "GET" && url.endsWith(`/issues/${ISSUE}/code-reviews/`)) return json(200, []);
    if (url.endsWith(`/api/workspaces/${SLUG}/members/`)) return json(200, []);
    if (url.endsWith("/api/instances/")) {
      return json(200, {
        config: {
          enable_signup: true,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: true,
          is_email_password_enabled: true,
          is_smtp_configured: false,
          is_self_managed: true,
          instance_changelog_url: "",
        },
      });
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url}`);
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <IssueDetailWidgets workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable />
    </QueryClientProvider>
  );
}

afterEach(() => resetAppClient());

describe("IssueDetailWidgets", () => {
  it("mounts the attachments strip and hides the code-review section at zero", async () => {
    renderWidgets();
    await waitFor(() => expect(screen.getByRole("region", { name: "Attachments" })).toBeInTheDocument());
    await waitFor(() => expect(screen.queryByText("Code reviews")).not.toBeInTheDocument());
    expect(screen.queryByTestId("worklog-edition-seam")).not.toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/work\s?log/i);
  });
});
