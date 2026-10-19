// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { PromptSectionListResponse, ResolvedPromptSection } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";
import { sessionKeys } from "../../../core/session/queries.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { listPromptSections: vi.fn(), getCompiledPrompt: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
}));

import { RECEIPTS_LOADING_TEXT, RECEIPTS_FAILED_TEXT } from "./ReceiptView.js";
import {
  PROMPTS_BASELINE_WARNING_TEXT,
  PROMPTS_LOAD_FAILED_TEXT,
  PROMPTS_LOADING_TEXT,
  PromptsView,
} from "./PromptsView.js";

const SLUG = "acme";

function row(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
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
    ...overrides,
  };
}

function listResponse(
  kind: string,
  scope: "user" | "workspace",
  sections: ResolvedPromptSection[]
): PromptSectionListResponse {
  return { kind, scope, sections };
}

function mockKinds(
  member: (kind: string) => ResolvedPromptSection[],
  baseline: (kind: string) => ResolvedPromptSection[] = () => []
): void {
  contractMocks.listPromptSections.mockImplementation(
    (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
      const kind = options.kind ?? "coding-task";
      if (options.scope === "workspace") return Promise.resolve(listResponse(kind, "workspace", baseline(kind)));
      return Promise.resolve(listResponse(kind, "user", member(kind)));
    }
  );
}

function seedRole(queryClient: QueryClient, role: number): void {
  queryClient.setQueryData(sessionKeys.workspaces, [
    {
      id: "123e4567-e89b-12d3-a456-426614174000",
      name: "Acme",
      slug: SLUG,
      logo_url: null,
      total_members: 3,
      role,
    },
  ]);
}

function mockCompiled(): void {
  contractMocks.getCompiledPrompt.mockImplementation((_client: unknown, _slug: string, kind: string) =>
    Promise.resolve({ kind, scope: "user", template_body: `${kind} assembled` })
  );
}

function renderView(role: number): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  seedRole(queryClient, role);
  render(
    <QueryClientProvider client={queryClient}>
      <PromptsView workspaceSlug={SLUG} />
    </QueryClientProvider>
  );
  return queryClient;
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCompiled();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("PromptsView", () => {
  it("renders the browser with nav anchors and cards for the admin", async () => {
    mockKinds(
      (kind) =>
        kind === "coding-task"
          ? [row({ key: "aaa", title: "Aaa" }), row({ key: "bbb", title: "Bbb" })]
          : [row({ key: "bbb", title: "Bbb" })],
      () => [row({ key: "aaa", title: "Aaa" }), row({ key: "bbb", title: "Bbb" })]
    );
    renderView(ROLE_ADMIN);
    expect(screen.getByRole("heading", { name: "Prompts" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Aaa" })).toBeInTheDocument());
    const nav = screen.getByRole("navigation", { name: "Prompt sections" });
    expect(within(nav).getByRole("link", { name: /Aaa/ })).toHaveAttribute("href", "#prompt-section-aaa");
    expect(within(nav).getByRole("link", { name: /Bbb/ })).toHaveAttribute("href", "#prompt-section-bbb");
    expect(document.querySelector("article#prompt-section-aaa")).not.toBeNull();
    expect(screen.getAllByRole("button", { name: "Customize for workspace" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Customize for me" })).toHaveLength(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows the loading line while the member lists resolve", () => {
    contractMocks.listPromptSections.mockReturnValue(new Promise<never>(() => {}));
    renderView(ROLE_ADMIN);
    expect(screen.getByText(PROMPTS_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Prompt sections" })).not.toBeInTheDocument();
  });

  it("shows the loading line while the admin baseline resolves", async () => {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        if (options.scope === "workspace") return new Promise<never>(() => {});
        return Promise.resolve(listResponse(options.kind ?? "coding-task", "user", [row()]));
      }
    );
    const queryClient = renderView(ROLE_ADMIN);
    // Wait for all member reads to settle: the remaining loading state
    // must come from the workspace baseline, rather than the first render.
    await waitFor(() =>
      expect(queryClient.isFetching({ predicate: (query) => query.queryKey.at(-1) === "user" })).toBe(0)
    );
    await waitFor(() => expect(screen.getByText(PROMPTS_LOADING_TEXT)).toBeInTheDocument());
    expect(screen.queryByRole("heading", { name: "Autonomy" })).not.toBeInTheDocument();
  });

  it("banners a member-list failure with a retry", async () => {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        if (options.scope === "user") return Promise.reject(new Error("boom"));
        return Promise.resolve(listResponse(options.kind ?? "coding-task", "workspace", [row()]));
      }
    );
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(PROMPTS_LOAD_FAILED_TEXT));
    expect(screen.queryByRole("navigation", { name: "Prompt sections" })).not.toBeInTheDocument();
    const calls = contractMocks.listPromptSections.mock.calls.length;
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        const kind = options.kind ?? "coding-task";
        const scope = options.scope === "workspace" ? "workspace" : "user";
        return Promise.resolve(listResponse(kind, scope as "user" | "workspace", [row()]));
      }
    );
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(contractMocks.listPromptSections.mock.calls.length).toBeGreaterThan(calls));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
  });

  it("warns the admin on baseline failure and hides workspace editing", async () => {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        if (options.scope === "workspace") return Promise.reject(new Error("boom"));
        return Promise.resolve(listResponse(options.kind ?? "coding-task", "user", [row()]));
      }
    );
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(PROMPTS_BASELINE_WARNING_TEXT));
    expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /workspace/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Customize for me" })).toBeInTheDocument();
  });

  it("gives members personal-only editing without fetching the baseline", async () => {
    mockKinds(() => [row()]);
    renderView(ROLE_MEMBER);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: /workspace/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Customize for me" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    for (const call of contractMocks.listPromptSections.mock.calls) {
      expect(call[2]).toMatchObject({ scope: "user" });
    }
  });

  it("gives guests a read-only browser", async () => {
    mockKinds(() => [row()]);
    renderView(ROLE_GUEST);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("effective body")).toBeInTheDocument();
  });

  it("opens and closes the editor on a card", async () => {
    mockKinds(
      () => [row()],
      () => [row()]
    );
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Customize for me" }));
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText("Prompt section text")).not.toBeInTheDocument());
    expect(screen.getByText("effective body")).toBeInTheDocument();
  });

  it("switches to the receipt tab and back, closing any open editor", async () => {
    mockKinds(
      () => [row()],
      () => [row()]
    );
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    expect(screen.getByRole("tab", { name: "Sections" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "Customize for me" }));
    expect(screen.getByLabelText("Prompt section text")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Receipt" }));
    expect(screen.getByRole("tab", { name: "Receipt" })).toHaveAttribute("aria-selected", "true");
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(3));
    expect(screen.getByRole("navigation", { name: "Prompt receipt" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Sections" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    expect(screen.queryByLabelText("Prompt section text")).not.toBeInTheDocument();
  });

  it("keeps the tabs while the receipt tab loads or fails", async () => {
    contractMocks.getCompiledPrompt.mockReturnValue(new Promise<never>(() => {}));
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("tab", { name: "Receipt" }));
    expect(screen.getByText(RECEIPTS_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Receipt" })).toBeInTheDocument();
  });

  it("shows the receipt failure banner with the tabs still reachable", async () => {
    contractMocks.getCompiledPrompt.mockRejectedValue(new Error("boom"));
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("tab", { name: "Receipt" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(RECEIPTS_FAILED_TEXT));
    expect(screen.getByRole("tab", { name: "Sections" })).toBeInTheDocument();
  });

  it("scopes the open editor to one card", async () => {
    const second = { ...row(), key: "blocking", title: "Blocking", body: "second body" };
    mockKinds(
      () => [row(), second],
      () => [row(), second]
    );
    renderView(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Blocking" })).toBeInTheDocument());
    const firstCard = document.querySelector("article#prompt-section-autonomy");
    if (firstCard === null) throw new Error("expected the autonomy card");
    fireEvent.click(within(firstCard as HTMLElement).getByRole("button", { name: "Customize for me" }));
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body");
    expect(screen.getByText("second body")).toBeInTheDocument();
  });
});
