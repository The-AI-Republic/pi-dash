// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ResolvedPromptSection } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { MergedPromptSection, PromptEditorScope, PromptViewerGates } from "../lib/sections.js";
import { SectionCard, STALE_OVERRIDE_WARNING } from "./SectionCard.js";

const SLUG = "acme";
const ADMIN: PromptViewerGates = { isAdmin: true, canEdit: true, baselineLoaded: true };

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

function section(overrides: Partial<MergedPromptSection> = {}): MergedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    kinds: ["coding-task", "review"],
    user: row(),
    workspace: row({ source: "default", body: "baseline body" }),
    ...overrides,
  };
}

function renderCard(
  props: {
    section?: MergedPromptSection;
    viewer?: PromptViewerGates;
    editorScope?: PromptEditorScope | null;
  } = {}
) {
  const onEdit = vi.fn();
  const onCloseEditor = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SectionCard
        workspaceSlug={SLUG}
        section={props.section ?? section()}
        viewer={props.viewer ?? ADMIN}
        editorScope={props.editorScope ?? null}
        onEdit={onEdit}
        onCloseEditor={onCloseEditor}
      />
    </QueryClientProvider>
  );
  return { onEdit, onCloseEditor };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("SectionCard", () => {
  it("renders the title, key, badges and effective body", () => {
    const { container } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section()}
          viewer={ADMIN}
          editorScope={null}
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument();
    expect(screen.getByText("autonomy")).toBeInTheDocument();
    expect(screen.getByText("Pi Dash default")).toBeInTheDocument();
    expect(screen.getByText("Coding task")).toBeInTheDocument();
    expect(screen.getByText("Review")).toBeInTheDocument();
    expect(container.querySelector("article#prompt-section-autonomy")).not.toBeNull();
    expect(screen.getByText("effective body")).toBeInTheDocument();
  });

  it("badges the override rungs", () => {
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section({ user: row({ source: "workspace" }) })}
          viewer={ADMIN}
          editorScope={null}
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByText("Workspace override")).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section({ user: row({ source: "user:abc" }) })}
          viewer={ADMIN}
          editorScope={null}
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByText("Your override")).toBeInTheDocument();
  });

  it("marks locked and admin-managed tiers", () => {
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section({ user: row({ customizable: "locked" }) })}
          viewer={ADMIN}
          editorScope={null}
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByText("Locked")).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section({ user: row({ customizable: "workspace" }) })}
          viewer={ADMIN}
          editorScope={null}
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.getByText("Admin-managed")).toBeInTheDocument();
  });

  it("warns on stale overrides only", () => {
    renderCard({ section: section({ user: row({ needs_attention: true }) }) });
    expect(screen.getByText(STALE_OVERRIDE_WARNING)).toBeInTheDocument();
  });

  it("offers both scopes and opens the editor at each", () => {
    const { onEdit } = renderCard();
    fireEvent.click(screen.getByRole("button", { name: "Customize for workspace" }));
    expect(onEdit).toHaveBeenCalledWith("workspace");
    fireEvent.click(screen.getByRole("button", { name: "Customize for me" }));
    expect(onEdit).toHaveBeenCalledWith("user");
  });

  it("offers workspace editing alone on an admin-managed section", () => {
    const { onEdit } = renderCard({
      section: section({ user: row({ customizable: "workspace", editable_at_personal: false }) }),
    });
    expect(screen.getByText("Admin-managed")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Customize for me" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Customize for workspace" }));
    expect(onEdit).toHaveBeenCalledWith("workspace");
  });

  it("hides the edit row when nothing is editable", () => {
    renderCard({ viewer: { isAdmin: false, canEdit: false, baselineLoaded: false } });
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("swaps the body for the editor while editing", () => {
    const { container } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SectionCard
          workspaceSlug={SLUG}
          section={section()}
          viewer={ADMIN}
          editorScope="user"
          onEdit={vi.fn()}
          onCloseEditor={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(container.querySelector("article pre")).toBeNull();
    expect(screen.getByLabelText("Prompt section text")).toHaveValue("effective body");
    expect(screen.queryByRole("button", { name: "Customize for me" })).not.toBeInTheDocument();
  });

  it("renders a badgeless empty card without an effective row", () => {
    renderCard({ section: section({ user: null }) });
    expect(screen.getByRole("heading", { name: "Autonomy" })).toBeInTheDocument();
    expect(screen.queryByText("Pi Dash default")).not.toBeInTheDocument();
    expect(screen.queryByText(STALE_OVERRIDE_WARNING)).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("wraps long body lines so wide sections cannot shove the side nav out of view", () => {
    // Regression (NEWFRONT-1042): registry bodies carry very long lines;
    // an unwrapped pre widened the cards track past the viewport, and the
    // side nav's anchor jump then scrolled the nav itself out of sight.
    renderCard({ section: section({ user: row({ body: `word ${"x".repeat(2000)}` }) }) });
    const pre = document.querySelector("article pre");
    expect(pre).not.toBeNull();
    expect(pre?.className).toContain("whitespace-pre-wrap");
  });
});
