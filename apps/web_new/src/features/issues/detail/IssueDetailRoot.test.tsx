// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { resetRegisteredStores } from "../../../core/session/index.js";
import { IssueDetailRoot } from "./IssueDetailRoot.js";
import { TITLE_LABEL } from "./IssueTitleField.js";
import { useDetailUiStore } from "./store.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function issueFixture(): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Current words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function stubViewport(narrow: boolean): void {
  vi.stubGlobal("matchMedia", () => ({
    matches: narrow,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

function stubViewportWithDispatch(narrow: boolean): { widen: () => void } {
  const listeners = new Set<(event: { matches: boolean }) => void>();
  const api = {
    matches: narrow,
    addEventListener: (_event: string, listener: (event: { matches: boolean }) => void) => {
      listeners.add(listener);
    },
    removeEventListener: (_event: string, listener: (event: { matches: boolean }) => void) => {
      listeners.delete(listener);
    },
  };
  vi.stubGlobal("matchMedia", () => api);
  return {
    widen: () => {
      api.matches = false;
      listeners.forEach((listener) => listener({ matches: false }));
    },
  };
}

function renderRoot(props: Partial<React.ComponentProps<typeof IssueDetailRoot>> = {}) {
  createAppClient("https://api.test", async (url: string) => {
    let status = 404;
    let body = "{}";
    if (url.includes("description-versions")) {
      status = 200;
      body = JSON.stringify({
        prev_cursor: "",
        cursor: "",
        next_cursor: null,
        prev_page_results: false,
        next_page_results: false,
        page_count: 0,
        total_results: 0,
        total_pages: 0,
        results: [],
      });
    } else if (url.endsWith("/members/")) {
      status = 200;
      body = JSON.stringify([]);
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <IssueDetailRoot
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        editable
        sidebar={<p>sidebar rows</p>}
        {...props}
      />
    </QueryClientProvider>
  );
}

afterEach(() => {
  // Unmount first: the store reset re-renders subscribers, and the
  // re-rendered column reads the client this hook just dropped.
  cleanup();
  vi.unstubAllGlobals();
  resetAppClient();
  resetRegisteredStores();
});

describe("IssueDetailRoot", () => {
  it("lays the main column beside the sidebar with its slots", () => {
    stubViewport(false);
    renderRoot({ topSlot: <p>header slot</p>, activity: <p>widget slot</p> });
    expect(screen.getByRole("textbox", { name: TITLE_LABEL })).toHaveValue("Fix login");
    expect(screen.getByRole("complementary", { name: "Properties" })).toHaveTextContent("sidebar rows");
    expect(screen.getByText("header slot")).toBeInTheDocument();
    expect(screen.getByText("widget slot")).toBeInTheDocument();
  });

  it("collapses and expands the sidebar from its toggle and rail", async () => {
    const user = userEvent.setup();
    stubViewport(false);
    renderRoot();
    const aside = () => screen.getByRole("complementary", { name: "Properties" });
    expect(aside()).not.toHaveClass("md:hidden");
    expect(screen.queryByRole("button", { name: "Expand properties" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Collapse properties" }));
    expect(aside()).toHaveClass("md:hidden");
    await user.click(screen.getByRole("button", { name: "Expand properties" }));
    expect(aside()).not.toHaveClass("md:hidden");
  });

  it("mounts no sidebar chrome without sidebar content", () => {
    stubViewport(false);
    renderRoot({ sidebar: undefined });
    expect(screen.queryByRole("complementary", { name: "Properties" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Expand properties" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: TITLE_LABEL })).toBeInTheDocument();
  });

  it("auto-collapses on narrow viewports", () => {
    stubViewport(true);
    renderRoot();
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(true);
    // Narrow viewports keep the content inline: the flag hides the
    // desktop rail, never the inline sidebar itself.
    expect(screen.getByRole("complementary", { name: "Properties" })).not.toHaveClass("hidden");
  });

  it("re-expands a collapsed sidebar when the viewport widens", () => {
    const viewport = stubViewportWithDispatch(true);
    renderRoot();
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(true);
    act(() => {
      viewport.widen();
    });
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(false);
    expect(screen.getByRole("complementary", { name: "Properties" })).not.toHaveClass("md:hidden");
  });

  it("keeps a user-chosen collapse on wide viewports", () => {
    stubViewport(false);
    useDetailUiStore.getState().setSidebarCollapsed(true);
    renderRoot();
    expect(useDetailUiStore.getState().sidebarCollapsed).toBe(true);
    expect(screen.getByRole("complementary", { name: "Properties" })).toHaveClass("md:hidden");
  });

  it("mounts no parent banner for an item without a parent", () => {
    stubViewport(false);
    renderRoot();
    expect(screen.queryByTestId("parent-banner")).not.toBeInTheDocument();
  });

  it("skips the parent banner when a host asks it to", () => {
    stubViewport(false);
    renderRoot({ showParentBanner: false });
    expect(screen.queryByTestId("parent-banner")).not.toBeInTheDocument();
  });

  it("reads only for guests, with no history line", () => {
    stubViewport(false);
    renderRoot({ editable: false });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Edited by/ })).not.toBeInTheDocument();
  });
});
