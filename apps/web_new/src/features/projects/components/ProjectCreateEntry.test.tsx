// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Deferred project-creation entry (shell spec, SHELL-086).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../api/keys.js";
import { ProjectCreateEntry } from "./ProjectCreateEntry.js";

describe("ProjectCreateEntry", () => {
  it("renders the creation dialog with the member directory", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/members/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify([
              {
                id: "123e4567-e89b-12d3-a456-426614174000",
                member: {
                  id: "123e4567-e89b-12d3-a456-426614174000",
                  first_name: "Ada",
                  last_name: "L",
                  avatar: "",
                  avatar_url: null,
                  display_name: "Ada",
                  is_bot: false,
                },
                role: 15,
              },
            ]),
        };
      }
      return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
    try {
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const onOpenChange = vi.fn();
      render(
        <QueryClientProvider client={queryClient}>
          <ProjectCreateEntry workspaceSlug="acme" open onOpenChange={onOpenChange} />
        </QueryClientProvider>
      );
      expect(await screen.findByRole("heading", { name: "New project" })).not.toBeNull();
      await waitFor(() => expect(queryClient.getQueryState(projectKeys.members("acme"))?.status).toBe("success"));
    } finally {
      resetAppClient();
    }
  });
});
