// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt view (parity/specs/agents.md §7, AGT-028, AGT-030, AGT-032).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CompiledPrompt, PromptSectionListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getCompiledPrompt: vi.fn(), listPromptSections: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { ReceiptView, RECEIPTS_FAILED_TEXT, RECEIPTS_LOADING_TEXT } from "./ReceiptView.js";

const KINDS = ["coding-task", "review", "scheduler"] as const;

function listed(kind: string, scope: "user" | "workspace"): PromptSectionListResponse {
  const count = kind === "coding-task" ? 2 : 1;
  return {
    kind,
    scope,
    sections: Array.from({ length: count }, (_, index) => ({
      key: `${kind}-${index}`,
      title: `${kind} section ${index}`,
      customizable: "overridable" as const,
      body: "body",
      default_body: "default",
      source: "default",
      version: 1,
      needs_attention: false,
      editable_at_workspace: true,
      editable_at_personal: true,
    })),
  };
}

function compiled(kind: string): CompiledPrompt {
  return {
    kind,
    scope: "user",
    template_body: `${kind} assembled`,
    ...(kind === "review" ? { automatic_template_body: `${kind} automatic` } : {}),
  };
}

function mockReads(options: { compiledFails?: boolean; sectionsFails?: boolean; pending?: boolean } = {}): void {
  contractMocks.getCompiledPrompt.mockImplementation((_client: unknown, _slug: string, kind: string) =>
    options.pending
      ? new Promise<never>(() => {})
      : options.compiledFails
        ? Promise.reject(new Error("compiled read failed"))
        : Promise.resolve(compiled(kind))
  );
  contractMocks.listPromptSections.mockImplementation(
    (_client: unknown, _slug: string, request: { kind?: string; scope?: string }) =>
      options.pending
        ? new Promise<never>(() => {})
        : options.sectionsFails
          ? Promise.reject(new Error("sections read failed"))
          : Promise.resolve(listed(request.kind ?? "coding-task", request.scope === "workspace" ? "workspace" : "user"))
  );
}

function renderView(isAdmin = true): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ReceiptView workspaceSlug="acme" isAdmin={isAdmin} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("ReceiptView", () => {
  it("shows a loading line until the receipt reads resolve", () => {
    mockReads({ pending: true });
    renderView();
    expect(screen.getByText(RECEIPTS_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Prompt receipt" })).not.toBeInTheDocument();
  });

  it("renders one card per kind with a count nav and anchors", async () => {
    mockReads();
    renderView();
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(3));
    const nav = screen.getByRole("navigation", { name: "Prompt receipt" });
    const links = within(nav).getAllByRole("link");
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "#prompt-receipt-coding-task",
      "#prompt-receipt-review",
      "#prompt-receipt-scheduler",
    ]);
    expect(within(nav).getByRole("link", { name: /Coding task/ })).toHaveTextContent("2 sections");
    expect(within(nav).getByRole("link", { name: /Review/ })).toHaveTextContent("1 section");
    expect(links.map((link) => link.textContent)).not.toContain("{{count}} sections");
  });

  it("banners a failed receipt read instead of blanking, and retries", async () => {
    mockReads({ compiledFails: true });
    renderView();
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(RECEIPTS_FAILED_TEXT));
    expect(screen.queryByRole("navigation", { name: "Prompt receipt" })).not.toBeInTheDocument();
    mockReads();
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(3));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("banners a failed section list on the receipt side too", async () => {
    mockReads({ sectionsFails: true });
    renderView();
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(RECEIPTS_FAILED_TEXT));
  });

  it("hides the saved preview for members and keeps the receipts readable", async () => {
    mockReads();
    renderView(false);
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(3));
    await userEvent.setup().click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.getByTestId("receipt-template-coding-task")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Preview" })).not.toBeInTheDocument();
  });

  it("reads the viewer's own compiled scope for every kind", async () => {
    mockReads();
    renderView();
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(3));
    for (const kind of KINDS) {
      expect(contractMocks.getCompiledPrompt).toHaveBeenCalledWith(expect.anything(), "acme", kind, {
        scope: "user",
      });
    }
  });
});
