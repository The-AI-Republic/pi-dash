// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression for NEWFRONT-900: a save rejected by the real API client must
// keep the editor mounted with its draft and show the server detail inline.
// The sibling component tests mock the contract function and so only prove a
// rejected promise; this test drives the actual ApiError path from a 500.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { MergedPromptSection } from "../lib/sections.js";
import { SectionEditor } from "./SectionEditor.js";

const SLUG = "acme";

const ROW = {
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
} as const;

const SECTION: MergedPromptSection = {
  key: "autonomy",
  title: "Autonomy",
  kinds: ["coding-task"],
  user: ROW,
  workspace: ROW,
};

function jsonResponse(status: number, body: unknown) {
  return {
    status,
    url: "https://api.test/api/workspaces/acme/prompt-sections/autonomy",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(body),
  };
}

afterEach(() => resetAppClient());

describe("SectionEditor with the real API client", () => {
  it("keeps the editor and draft open when the server answers 500", async () => {
    const transport: Transport = async (_url, init) =>
      init.method === "PUT"
        ? jsonResponse(500, { error: "parity upsert failure" })
        : jsonResponse(200, { kind: "coding-task", scope: "user", sections: [ROW] });
    createAppClient("https://api.test", transport);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });

    const onClose = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <SectionEditor workspaceSlug={SLUG} section={SECTION} scope="user" onClose={onClose} />
      </QueryClientProvider>
    );
    fireEvent.change(screen.getByLabelText("Prompt section text"), { target: { value: "never persisted" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Could not save the section. parity upsert failure")
    );
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("never persisted");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
