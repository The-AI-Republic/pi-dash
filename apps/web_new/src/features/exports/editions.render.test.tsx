// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// EXP-024 at the surface level (NEWFRONT-773): the same export components
// render ungated when the app is built with the cloud edition module. The
// surfaces never read the edition, so a cloud build shows the same form,
// history, analytics action, profile download and dialog with no gate
// marker. Desktop shares this source tree (one build mode), which the
// source sweep pins.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

// A cloud-shaped edition: extra routes, sidebar entries and slots that a
// gate-happy implementation might have used to hide export behavior.
vi.mock("@pidash/edition", () => ({
  edition: {
    id: "cloud",
    flags: { "app-rail": true, "sidebar-toggle": true },
    auth: [],
    routes: [{ path: "apps" }],
    sidebar: [{ id: "apps", label: "Apps", to: "apps" }],
    settingsSections: [{ id: "billing", label: "Billing" }],
    slots: {},
  },
}));

import { AnalyticsTable } from "../analytics/components/AnalyticsTable.js";
import { ExportsForm } from "./components/ExportsForm.js";
import { ExportsHistory } from "./components/ExportsHistory.js";
import { ProfileActivityDownload } from "../profile/components/ProfileActivityDownload.js";
import { PageExportDialog } from "../pages-export/components/PageExportDialog.js";
import type { PageExportHost } from "../pages-export/lib/pageExportDialogRegistry.js";

const PROJECT = {
  id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  name: "Exp Alpha",
  identifier: "EA",
  sort_order: null,
  logo_props: {},
  member_role: 15,
  intake_count: 0,
  archived_at: null,
  workspace: "44444444-4444-4444-8444-444444444444",
  cycle_view: false,
  issue_views_view: false,
  module_view: false,
  page_view: false,
  inbox_view: false,
  is_default: false,
  guest_view_all_features: false,
  project_lead: null,
  network: 0,
  created_at: "2026-01-01T00:00:00.000Z",
  updated_at: "2026-01-01T00:00:00.000Z",
  created_by: null,
  updated_by: null,
};

const HOST: PageExportHost = {
  pageTitle: "Export me",
  projectId: "project-1",
  workspaceSlug: "acme",
  getHtml: () => "<p>body</p>",
  getMarkdown: () => "body",
};

/** No gate wording may reach a rendered export surface. */
const GATE_MARKERS =
  /\b(upgrade to|upgrade required|paywall|desktop-only|cloud-only|license required|licence required)\b/i;

function answer(url: string, body: unknown) {
  return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
}

function stubTransport(): void {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return answer(url, { csrf_token: "csrf" });
    if (url.endsWith("/projects/")) return answer(url, [PROJECT]);
    if (url.includes("/export-issues"))
      return answer(url, {
        grouped_by: null,
        sub_grouped_by: null,
        total_count: 0,
        next_cursor: "10:1:0",
        prev_cursor: "10:-1:1",
        next_page_results: false,
        prev_page_results: false,
        count: 0,
        total_pages: 0,
        total_results: 0,
        extra_stats: null,
        results: [],
      });
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
}

function wrap(node: React.ReactNode): React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={client}>{node}</QueryClientProvider>;
}

afterEach(() => {
  resetAppClient();
});

describe("export surfaces under the cloud edition (EXP-024)", () => {
  it("renders the workspace export form and history ungated", async () => {
    stubTransport();
    render(
      wrap(
        <>
          <ExportsForm workspaceSlug="acme" />
          <ExportsHistory workspaceSlug="acme" />
        </>
      )
    );
    expect(screen.getByRole("heading", { name: "Exporting project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Previous exports" })).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(GATE_MARKERS);
  });

  it("renders the analytics export action ungated", () => {
    render(
      <AnalyticsTable
        testId="cloud-analytics-table"
        columns={[{ key: "name", label: "Name", align: "left", text: (row: { name: string }) => row.name }]}
        rows={[{ name: "Urgent" }]}
        countNoun="Items"
        onExport={() => undefined}
      />
    );
    expect(screen.getByRole("button", { name: "Export as csv" })).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(GATE_MARKERS);
  });

  it("renders the profile download ungated", () => {
    render(<ProfileActivityDownload workspaceSlug="acme" userId="u-1" run={async () => undefined} />);
    expect(screen.getByRole("button", { name: /Download today's activity/ })).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(GATE_MARKERS);
  });

  it("renders the page-export dialog ungated", () => {
    render(
      <PageExportDialog host={HOST} open onClose={() => undefined} saveFile={async () => ({ filename: "x.pdf" })} />
    );
    expect(screen.getByRole("dialog", { name: "Export page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(GATE_MARKERS);
  });
});
