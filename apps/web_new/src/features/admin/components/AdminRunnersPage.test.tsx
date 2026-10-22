// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners list page: header, loading skeleton, runners table with
// the filter and total count, the empty message row, the load-failure
// retry, and the prev/next pager. Reads run through the real app
// client against a stub transport so the filter/page query params are
// pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { RUNNERS_DESCRIPTION, RUNNERS_EMPTY_TEXT, RUNNERS_HEADERS, RUNNERS_HEADING } from "../runners.js";
import { AdminRunnersPage } from "./AdminRunnersPage.js";

function runnerRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "66666666-6666-4656-8666-666666666666",
    name: "Probe runner",
    workspace_slug: "probe-workspace",
    owner_email: "member@example.test",
    host_label: "probe-runner",
    provisioning: "manual",
    status: "online",
    runner_version: "1.2.3",
    codex_version: "0.44.0",
    last_heartbeat_at: "2026-02-03T04:05:06Z",
    created_at: "2026-02-01T00:00:00Z",
    revoked_at: null,
    ...overrides,
  };
}

interface RunnersHarness {
  urls: string[];
  failList: () => void;
  allowList: () => void;
}

async function renderPage(options: {
  pages: Record<string, { page: number; total: number; results: unknown[] }>;
  listFails?: boolean;
  listGate?: Promise<void>;
}): Promise<RunnersHarness> {
  const urls: string[] = [];
  let listFails = options.listFails ?? false;
  const listGate = options.listGate ?? Promise.resolve();
  const transport: Transport = async (url, init) => {
    const parsed = new URL(url, "https://api.test");
    if (init.method === "GET" && parsed.pathname === "/api/instances/runners/") {
      urls.push(url);
      await listGate;
      if (listFails) {
        return {
          status: 500,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
        };
      }
      const key = `${parsed.searchParams.get("provisioning") ?? ""}|${parsed.searchParams.get("page") ?? "1"}`;
      const body = options.pages[key] ?? { page: 1, total: 0, results: [] };
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(body)),
      };
    }
    throw new Error(`unexpected request ${init.method} ${url}`);
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminRunnersPage />
    </QueryClientProvider>
  );
  await screen.findByTestId("runners-page");
  return {
    urls,
    failList: () => {
      listFails = true;
    },
    allowList: () => {
      listFails = false;
    },
  };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminRunnersPage", () => {
  it("shows the header while the list loads", async () => {
    let release!: () => void;
    const listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ pages: { "|1": { page: 1, total: 1, results: [runnerRow()] } }, listGate });
    expect(screen.getByRole("heading", { name: RUNNERS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(RUNNERS_DESCRIPTION)).toBeInTheDocument();
    release();
    await screen.findByTestId("runners-table");
  });

  it("shows two 48px skeleton bars with no table while loading", async () => {
    let release!: () => void;
    const listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ pages: { "|1": { page: 1, total: 1, results: [runnerRow()] } }, listGate });
    const skeleton = screen.getByTestId("runners-skeleton");
    expect(skeleton.getAttribute("role")).toBe("status");
    const bars = skeleton.querySelectorAll(":scope > *");
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.height).toBe("48px");
    expect((bars[1] as HTMLElement).style.height).toBe("48px");
    expect(screen.queryByTestId("runners-table")).not.toBeInTheDocument();
    expect(screen.queryByTestId("runners-filter")).not.toBeInTheDocument();
    release();
    await screen.findByTestId("runners-table");
  });

  it("renders one eight-cell row per runner with headers", async () => {
    await renderPage({
      pages: {
        "|1": {
          page: 1,
          total: 2,
          results: [
            runnerRow(),
            runnerRow({
              id: "77777777-7777-4757-8777-777777777777",
              name: "Second runner",
              provisioning: "desktop_bundled",
              status: "busy",
            }),
          ],
        },
      },
    });
    const table = await screen.findByTestId("runners-table");
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent);
    expect(headers).toEqual([...RUNNERS_HEADERS]);
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    const first = within(rows[1] as HTMLElement).getAllByRole("cell");
    expect(first.map((cell) => cell.textContent)).toEqual([
      "Probe runner",
      "probe-workspace",
      "member@example.test",
      "manual",
      "online",
      "1.2.3",
      "0.44.0",
      new Date("2026-02-03T04:05:06Z").toLocaleString(),
    ]);
    expect(screen.getByTestId("runners-total")).toHaveTextContent("2 runners");
    expect(screen.queryByTestId("runners-empty")).not.toBeInTheDocument();
    expect(screen.queryByTestId("runners-pager")).not.toBeInTheDocument();
  });

  it("renders absent versions, slugs, owners, and heartbeats as an em dash", async () => {
    await renderPage({
      pages: {
        "|1": {
          page: 1,
          total: 1,
          results: [
            runnerRow({
              workspace_slug: null,
              owner_email: null,
              runner_version: "",
              codex_version: null,
              last_heartbeat_at: null,
            }),
          ],
        },
      },
    });
    const table = await screen.findByTestId("runners-table");
    const cells = within(within(table).getAllByRole("row")[1] as HTMLElement).getAllByRole("cell");
    expect(cells.map((cell) => cell.textContent)).toEqual([
      "Probe runner",
      "—",
      "—",
      "manual",
      "online",
      "—",
      "—",
      "—",
    ]);
    expect(screen.getByTestId("runners-total")).toHaveTextContent("1 runner");
  });

  it("keeps the headers with the empty message row when no runners match", async () => {
    await renderPage({ pages: { "|1": { page: 1, total: 0, results: [] } } });
    const table = await screen.findByTestId("runners-table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent)
    ).toEqual([...RUNNERS_HEADERS]);
    expect(screen.getByTestId("runners-empty")).toHaveTextContent(RUNNERS_EMPTY_TEXT);
    expect(screen.getByTestId("runners-total")).toHaveTextContent("0 runners");
    expect(screen.queryByTestId("runners-pager")).not.toBeInTheDocument();
  });

  it("retries the failed read from the error block", async () => {
    const harness = await renderPage({
      pages: { "|1": { page: 1, total: 1, results: [runnerRow()] } },
      listFails: true,
    });
    expect(await screen.findByTestId("runners-error")).toBeInTheDocument();
    expect(screen.queryByTestId("runners-table")).not.toBeInTheDocument();
    harness.allowList();
    await userEvent.setup().click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByTestId("runners-table");
    expect(harness.urls).toHaveLength(2);
  });

  it("offers the all/manual/desktop_bundled filter and refetches from page 1", async () => {
    const user = userEvent.setup();
    const pageTwo = {
      page: 2,
      total: 51,
      results: [runnerRow({ id: "88888888-8888-4858-8888-888888888888", name: "Page two runner" })],
    };
    const harness = await renderPage({
      pages: {
        "|1": { page: 1, total: 51, results: [runnerRow()] },
        "|2": pageTwo,
        "manual|1": { page: 1, total: 1, results: [runnerRow({ name: "Manual runner" })] },
      },
    });
    const filter = (await screen.findByTestId("runners-filter")) as HTMLSelectElement;
    expect([...filter.options].map((option) => option.value)).toEqual(["", "manual", "desktop_bundled"]);
    expect([...filter.options].map((option) => option.text)).toEqual(["all", "manual", "desktop_bundled"]);
    await user.click(screen.getByTestId("runners-next"));
    await screen.findByText("Page two runner");
    expect(harness.urls[harness.urls.length - 1]).toBe("https://api.test/api/instances/runners/?page=2");
    await user.selectOptions(screen.getByTestId("runners-filter"), "manual");
    await screen.findByText("Manual runner");
    expect(harness.urls[harness.urls.length - 1]).toBe(
      "https://api.test/api/instances/runners/?provisioning=manual&page=1"
    );
    expect(screen.getByTestId("runners-page")).toBeInTheDocument();
    expect(screen.queryByTestId("runners-pager")).not.toBeInTheDocument();
    expect(screen.getByTestId("runners-total")).toHaveTextContent("1 runner");
  });

  it("pages forward and back with the bounds disabled", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({
      pages: {
        "|1": { page: 1, total: 51, results: [runnerRow({ name: "First page runner" })] },
        "|2": {
          page: 2,
          total: 51,
          results: [runnerRow({ id: "99999999-9999-4959-8999-999999999999", name: "Last page runner" })],
        },
      },
    });
    await screen.findByText("First page runner");
    expect(screen.getByTestId("runners-page-label")).toHaveTextContent("Page 1");
    expect(screen.getByTestId("runners-prev")).toBeDisabled();
    expect(screen.getByTestId("runners-next")).toBeEnabled();
    await user.click(screen.getByTestId("runners-next"));
    await screen.findByText("Last page runner");
    expect(screen.getByTestId("runners-page-label")).toHaveTextContent("Page 2");
    expect(screen.getByTestId("runners-prev")).toBeEnabled();
    expect(screen.getByTestId("runners-next")).toBeDisabled();
    expect(harness.urls[harness.urls.length - 1]).toBe("https://api.test/api/instances/runners/?page=2");
    await user.click(screen.getByTestId("runners-prev"));
    await screen.findByText("First page runner");
    expect(screen.getByTestId("runners-page-label")).toHaveTextContent("Page 1");
  });
});
