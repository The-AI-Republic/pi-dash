// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Receipt card (parity/specs/agents.md §7, AGT-028-030).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CompiledPrompt, ResolvedPromptSection } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { previewPrompt: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import type { PromptReceiptEntry } from "../api/queries.js";
import { AUTOMATIC_RUNS_LABEL, ReceiptCard } from "./ReceiptCard.js";

function section(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    customizable: "overridable",
    body: "body",
    default_body: "default",
    source: "default",
    version: 1,
    needs_attention: false,
    editable_at_workspace: true,
    editable_at_personal: true,
    ...overrides,
  };
}

function receipt(overrides: Partial<PromptReceiptEntry> = {}): PromptReceiptEntry {
  const compiled: CompiledPrompt = {
    kind: "coding-task",
    scope: "user",
    template_body: "Assembled template.\n\nwith trailing newline\n",
  };
  return {
    kind: "coding-task",
    sections: {
      kind: "coding-task",
      sections: [
        section({ key: "intro", title: "Intro" }),
        section({ key: "autonomy", title: "Autonomy", source: "user:1" }),
      ],
    },
    compiled,
    ...overrides,
  };
}

function renderCard(props: { entry?: PromptReceiptEntry; isAdmin?: boolean } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ReceiptCard workspaceSlug="acme" receipt={props.entry ?? receipt()} isAdmin={props.isAdmin ?? false} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("ReceiptCard", () => {
  it("numbers the composition in order with provenance and a rendered count", () => {
    renderCard();
    const card = screen.getByRole("article");
    expect(card).toHaveAttribute("id", "prompt-receipt-coding-task");
    const header = within(card).getByRole("button", { name: "Coding task" });
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(within(card).getByTestId("receipt-count")).toHaveTextContent("2 sections");
    expect(screen.queryByText("{{count}} sections")).not.toBeInTheDocument();
    const items = within(card).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(within(items[0] as HTMLElement).getByText("01")).toBeInTheDocument();
    expect(within(items[0] as HTMLElement).getByText("Intro")).toBeInTheDocument();
    expect(within(items[0] as HTMLElement).getByText("Pi Dash default")).toBeInTheDocument();
    expect(within(items[1] as HTMLElement).getByText("02")).toBeInTheDocument();
    expect(within(items[1] as HTMLElement).getByText("Your override")).toBeInTheDocument();
  });

  it("uses singular copy for a one-section receipt", () => {
    renderCard({
      entry: receipt({
        sections: { kind: "review", sections: [section({ key: "only" })] },
        compiled: { kind: "review", scope: "user", template_body: "one" },
      }),
    });
    expect(screen.getByTestId("receipt-count")).toHaveTextContent("1 section");
  });

  it("expands to the assembled template with trailing blank lines trimmed and collapses again", async () => {
    const user = userEvent.setup();
    renderCard();
    expect(screen.queryByTestId("receipt-template-coding-task")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.getByRole("button", { name: "Coding task" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("receipt-template-coding-task").textContent).toBe(
      "Assembled template.\n\nwith trailing newline"
    );
    await user.click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.queryByTestId("receipt-template-coding-task")).not.toBeInTheDocument();
  });

  it("contrasts the automatic-run variant when the compiled read carries one", async () => {
    const user = userEvent.setup();
    renderCard({
      entry: receipt({
        compiled: {
          kind: "coding-task",
          scope: "user",
          template_body: "personal assembly",
          automatic_template_body: "automatic assembly\n",
        },
      }),
    });
    await user.click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.getByText(AUTOMATIC_RUNS_LABEL)).toBeInTheDocument();
    expect(screen.getByTestId("receipt-automatic-coding-task").textContent).toBe("automatic assembly");
  });

  it("shows a single block when no overrides exist", async () => {
    const user = userEvent.setup();
    renderCard();
    await user.click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.queryByText(AUTOMATIC_RUNS_LABEL)).not.toBeInTheDocument();
    expect(screen.queryByTestId("receipt-automatic-coding-task")).not.toBeInTheDocument();
  });

  it("shows the saved-preview form to admins only, and never while collapsed", async () => {
    renderCard({ isAdmin: false });
    await userEvent.setup().click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.queryByRole("heading", { name: "Preview" })).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Issue id (UUID)")).not.toBeInTheDocument();
  });

  it("renders the admin preview form inside the expanded card", async () => {
    const user = userEvent.setup();
    renderCard({ isAdmin: true });
    expect(screen.queryByRole("heading", { name: "Preview" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Coding task" }));
    expect(screen.getByRole("heading", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Issue id (UUID)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
  });

  it("renders the scheduler kind with an install target", async () => {
    const user = userEvent.setup();
    renderCard({
      isAdmin: true,
      entry: receipt({
        kind: "scheduler",
        sections: { kind: "scheduler", sections: [section({ key: "brief" })] },
        compiled: { kind: "scheduler", scope: "user", template_body: "scheduler template" },
      }),
    });
    expect(screen.getByRole("article")).toHaveAttribute("id", "prompt-receipt-scheduler");
    await user.click(screen.getByRole("button", { name: "Scheduler" }));
    expect(screen.getByPlaceholderText("Scheduler install id (UUID)")).toBeInTheDocument();
  });
});
