// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompts home route (AGT-023). Thin mount: proves the route renders the
// connected sections browser for the workspace slug.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { PromptSectionListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../../core/session/index.js";
import { sessionKeys } from "../../../../core/session/queries.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { listPromptSections: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
}));

import { Route } from "./index.js";

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("prompts route", () => {
  it("mounts the sections browser for the workspace", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    contractMocks.listPromptSections.mockImplementation(
      (
        _client: unknown,
        _slug: string,
        options: { kind?: string; scope?: string }
      ): Promise<PromptSectionListResponse> => {
        const kind = options.kind ?? "coding-task";
        const scope = options.scope === "workspace" ? "workspace" : "user";
        return Promise.resolve({
          kind,
          scope,
          sections: [
            {
              key: "autonomy",
              title: "Autonomy",
              customizable: "overridable",
              body: "effective body",
              default_body: "pristine default",
              source: "default",
              version: 1,
              needs_attention: false,
              editable_at_workspace: true,
              editable_at_personal: true,
            },
          ],
        });
      }
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, [
      {
        id: "123e4567-e89b-12d3-a456-426614174000",
        name: "Acme",
        slug: "acme",
        logo_url: null,
        total_members: 1,
        role: ROLE_ADMIN,
      },
    ]);
    const Component = Route.options.component;
    if (!Component) throw new Error("prompts route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "Prompts" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    expect(contractMocks.listPromptSections).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      expect.objectContaining({ kind: "coding-task" })
    );
  });
});
