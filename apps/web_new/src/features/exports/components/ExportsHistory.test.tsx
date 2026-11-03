// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { exportsKeys } from "../api/keys.js";
import {
  EXPORT_HISTORY_EMPTY_DESCRIPTION,
  EXPORT_HISTORY_EMPTY_TITLE,
  EXPORT_HISTORY_ERROR_MESSAGE,
  EXPORT_HISTORY_LOADING_LABEL,
} from "../exportHistory.js";
import { expectContentKeysInert, expectKeyboardReachable, pressEnter, tabTo } from "../../../testSupport/keyboard.js";
import { ExportsHistory } from "./ExportsHistory.js";

const USER_ID = "11111111-1111-4111-8111-111111111111";

function uuid(n: number): string {
  return `12345678-1234-4234-8234-${String(n).padStart(12, "0")}`;
}

function hoursAgo(hours: number): string {
  return new Date(Date.now() - hours * 3_600_000).toISOString();
}

interface StubRecord {
  status?: string;
  provider?: string;
  url?: string | null;
  created_at?: string;
  project?: string[] | null;
  avatar_url?: string | null;
  display_name?: string;
}

function stubRecord(index: number, overrides: StubRecord = {}) {
  return {
    id: uuid(index),
    created_at: overrides.created_at ?? hoursAgo(1),
    updated_at: overrides.created_at ?? hoursAgo(1),
    project: overrides.project === undefined ? [uuid(900 + index)] : overrides.project,
    provider: overrides.provider ?? "csv",
    status: overrides.status ?? "completed",
    url: overrides.url === undefined ? `https://files.test/export-${index}.csv` : overrides.url,
    initiated_by: USER_ID,
    initiated_by_detail: {
      id: USER_ID,
      first_name: "Ana",
      last_name: "Siva",
      avatar: "",
      avatar_url: overrides.avatar_url ?? null,
      display_name: overrides.display_name ?? "Ana Siva",
      is_bot: false,
    },
    token: "token",
    created_by: null,
    updated_by: null,
  };
}

function stubPage(results: ReturnType<typeof stubRecord>[], paging: object = {}) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "10:1:0",
    prev_cursor: "10:-1:1",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_pages: 1,
    total_results: results.length,
    extra_stats: null,
    results,
    ...paging,
  };
}

function ok(body: unknown) {
  return {
    status: 200,
    url: "https://api.test/history",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(body),
  };
}

function failed() {
  return {
    status: 500,
    url: "https://api.test/history",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify({ error: "boom" }),
  };
}

/** Programmable history transport: answers, failures, and gates per test. */
function stubTransport(answer: () => Promise<unknown>): string[] {
  const urls: string[] = [];
  createAppClient("https://api.test", (async (url: string) => {
    urls.push(url);
    return answer();
  }) as never);
  return urls;
}

function renderHistory(slug = "acme"): QueryClient {
  const client = new QueryClient();
  render(<ExportsHistory workspaceSlug={slug} />, {
    wrapper: function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
      return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
    },
  });
  return client;
}

function historyRows(): HTMLElement[] {
  return within(screen.getByRole("table")).getAllByRole("row").slice(1);
}

afterEach(() => {
  resetAppClient();
});

describe("ExportsHistory", () => {
  it("shows a skeleton before the first response lands", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubTransport(() => gate.then(() => ok(stubPage([stubRecord(1)]))));
    renderHistory();

    expect(await screen.findByRole("status", { name: EXPORT_HISTORY_LOADING_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();

    await act(async () => {
      release();
    });
    expect(await screen.findByRole("table")).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: EXPORT_HISTORY_LOADING_LABEL })).not.toBeInTheDocument();
  });

  it("renders one row per record with initiator, date, count, format, and state", async () => {
    stubTransport(async () =>
      ok(
        stubPage([
          stubRecord(1, {
            status: "processing",
            provider: "xlsx",
            url: null,
            project: [uuid(1), uuid(2)],
            avatar_url: "https://files.test/ana.png",
          }),
        ])
      )
    );
    renderHistory();
    await screen.findByRole("table");

    const [row] = historyRows();
    expect(row).toBeDefined();
    if (!row) return;
    const cells = within(row).getAllByRole("cell");
    expect(cells).toHaveLength(6);
    expect(cells[0]?.textContent).toContain("Ana Siva");
    // Pinned for the parity driver: the image contributes no text, so the
    // cell reads as the bare display name.
    expect(cells[0]?.textContent).toBe("Ana Siva");
    expect(within(cells[0] as HTMLElement).getByRole("img", { name: "Ana Siva" })).toBeInTheDocument();
    expect(cells[0]?.querySelector("img")?.getAttribute("src")).toBe("https://files.test/ana.png");
    expect(cells[1]?.textContent).toMatch(/^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
    expect(cells[2]?.textContent).toBe("2 project(s)");
    expect(cells[3]?.textContent).toBe("Excel");
    expect(cells[4]?.textContent).toBe("processing");
    expect(cells[5]?.textContent).toBe("-");
  });

  it("shows the initial when the initiator has no avatar", async () => {
    stubTransport(async () => ok(stubPage([stubRecord(1, { display_name: "ana siva" })])));
    renderHistory();
    await screen.findByRole("table");

    const [row] = historyRows();
    expect(row).toBeDefined();
    if (!row) return;
    const first = within(row as HTMLElement).getAllByRole("cell")[0] as HTMLElement;
    expect(first.querySelector("img")).toBeNull();
    expect(within(first).getByRole("img", { name: "ana siva" })).toHaveTextContent("A");
    // Pinned for the parity driver: the glyph prefixes the name with no
    // separator, so the driver strips one character to recover it.
    expect(first.textContent).toBe("Aana siva");
  });

  it("marks queued, processing, completed, failed, and unknown states distinctly", async () => {
    stubTransport(async () =>
      ok(
        stubPage([
          stubRecord(1, { status: "queued", url: null }),
          stubRecord(2, { status: "processing", url: null }),
          stubRecord(3, { status: "completed" }),
          stubRecord(4, { status: "failed", url: null }),
          stubRecord(5, { status: "archived", url: null }),
        ])
      )
    );
    renderHistory();
    await screen.findByRole("table");

    const states = historyRows().map((row) => within(row).getAllByRole("cell")[4]?.textContent);
    expect(states).toEqual(["queued", "processing", "completed", "failed", "unknown"]);
  });

  it("labels csv, xlsx, json, and unknown providers", async () => {
    stubTransport(async () =>
      ok(
        stubPage([
          stubRecord(1, { provider: "csv" }),
          stubRecord(2, { provider: "xlsx" }),
          stubRecord(3, { provider: "json" }),
          stubRecord(4, { provider: "parquet" }),
        ])
      )
    );
    renderHistory();
    await screen.findByRole("table");

    const formats = historyRows().map((row) => within(row).getAllByRole("cell")[3]?.textContent);
    expect(formats).toEqual(["CSV", "Excel", "JSON", "Unknown"]);
  });

  it("links a fresh finished row to its file in a new tab", async () => {
    stubTransport(async () => ok(stubPage([stubRecord(1)])));
    renderHistory();
    await screen.findByRole("table");

    const link = screen.getByRole("link", { name: /Download export from / });
    expect(link).toHaveTextContent("Download");
    expect(link.getAttribute("href")).toBe("https://files.test/export-1.csv");
    expect(link.getAttribute("target")).toBe("_blank");
  });

  it("shows the expired marker past seven days, including exactly on the boundary", async () => {
    const boundary = new Date(Date.now() - 7 * 86_400_000).toISOString();
    stubTransport(async () =>
      ok(
        stubPage([
          stubRecord(1, { created_at: new Date(Date.now() - 9 * 86_400_000).toISOString() }),
          stubRecord(2, { created_at: boundary }),
          stubRecord(3, { created_at: hoursAgo(1) }),
        ])
      )
    );
    renderHistory();
    await screen.findByRole("table");

    const downloads = historyRows().map((row) => within(row).getAllByRole("cell")[5]?.textContent);
    expect(downloads).toEqual(["Expired", "Expired", "Download"]);
    expect(screen.getAllByRole("link", { name: /Download export from / })).toHaveLength(1);
  });

  it("shows the illustrated empty view with no table and no pagination", async () => {
    stubTransport(async () => ok(stubPage([])));
    renderHistory();

    expect(await screen.findByRole("heading", { name: new RegExp(EXPORT_HISTORY_EMPTY_TITLE) })).toBeInTheDocument();
    expect(screen.getByText(EXPORT_HISTORY_EMPTY_DESCRIPTION)).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Prev" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();
  });

  it("pages ten rows at a time with end-stopped controls following the envelope", async () => {
    const first = Array.from({ length: 10 }, (_, index) => stubRecord(index + 1));
    const urls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      urls.push(url);
      const cursor = new URL(url).searchParams.get("cursor");
      if (cursor === "10:1:0") {
        return ok(
          stubPage([stubRecord(11)], {
            next_cursor: "10:2:0",
            prev_cursor: "10:0:0",
            next_page_results: false,
            prev_page_results: true,
          })
        );
      }
      return ok(
        stubPage(first, {
          next_cursor: "10:1:0",
          prev_cursor: "10:-1:1",
          next_page_results: true,
          prev_page_results: false,
        })
      );
    }) as never);
    renderHistory();
    await screen.findByRole("table");

    expect(historyRows()).toHaveLength(10);
    expect(screen.getByRole("button", { name: "Prev" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(historyRows()).toHaveLength(1));
    expect(screen.getByRole("button", { name: "Prev" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    expect(urls[1]).toContain("cursor=10%3A1%3A0");

    fireEvent.click(screen.getByRole("button", { name: "Prev" }));
    await waitFor(() => expect(historyRows()).toHaveLength(10));
    expect(screen.getByRole("button", { name: "Prev" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    expect(urls[2]).toContain("cursor=10%3A0%3A0");
  });

  it("refreshes on demand with a working label while the re-read runs", async () => {
    let release!: () => void;
    let gate: Promise<void> | null = null;
    stubTransport(() => {
      const pending = gate;
      gate = null;
      const answer = pending === null ? Promise.resolve() : pending;
      return answer.then(() => ok(stubPage([stubRecord(1)])));
    });
    renderHistory();
    await screen.findByRole("table");

    const refresh = () => screen.getByRole("button", { name: /Refresh/ });
    expect(refresh()).toHaveTextContent("Refresh status");

    gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    fireEvent.click(refresh());
    expect(await screen.findByRole("button", { name: "Refreshing" })).toBeInTheDocument();

    await act(async () => {
      release();
    });
    expect(await screen.findByRole("button", { name: "Refresh status" })).toBeInTheDocument();
  });

  it("shows a fresh request after the history is invalidated", async () => {
    let count = 1;
    stubTransport(async () => ok(stubPage([stubRecord(1), stubRecord(2)].slice(0, count))));
    const client = renderHistory();
    await screen.findByRole("table");
    expect(historyRows()).toHaveLength(1);

    count = 2;
    await act(async () => {
      await client.invalidateQueries({ queryKey: exportsKeys.history("acme") });
    });
    await waitFor(() => expect(historyRows()).toHaveLength(2));
  });

  it("keeps the refresh usable when the read fails, and recovers on retry", async () => {
    let fail = true;
    stubTransport(async () => (fail ? failed() : ok(stubPage([stubRecord(1)]))));
    renderHistory();

    expect(await screen.findByText(EXPORT_HISTORY_ERROR_MESSAGE)).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();

    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
    expect(await screen.findByRole("table")).toBeInTheDocument();
    expect(screen.queryByText(EXPORT_HISTORY_ERROR_MESSAGE)).not.toBeInTheDocument();
  });

  it("reaches refresh and paging by Tab, activates them on Enter and leaves content keys inert", async () => {
    const user = userEvent.setup();
    const urls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      urls.push(url);
      const cursor = new URL(url).searchParams.get("cursor");
      if (cursor === "10:1:0") {
        return ok(
          stubPage([stubRecord(11)], {
            next_cursor: "10:2:0",
            prev_cursor: "10:0:0",
            next_page_results: false,
            prev_page_results: true,
          })
        );
      }
      return ok(
        stubPage(
          Array.from({ length: 10 }, (_, index) => stubRecord(index + 1)),
          {
            next_cursor: "10:1:0",
            prev_cursor: "10:-1:1",
            next_page_results: true,
            prev_page_results: false,
          }
        )
      );
    }) as never);
    renderHistory();
    await screen.findByRole("table");

    const refresh = screen.getByRole("button", { name: "Refresh status" });
    const next = screen.getByRole("button", { name: "Next" });
    const prev = screen.getByRole("button", { name: "Prev" });
    const controls = expectKeyboardReachable(screen.getByRole("region", { name: "Previous exports" }));
    // Prev starts disabled (first page); the reachable actions are refresh,
    // next and each row's download link.
    expect(controls).toEqual(expect.arrayContaining([refresh, next]));
    const downloads = screen.getAllByRole("link", { name: /^Download export from/ });
    expect(downloads).toHaveLength(10);
    expect(downloads[0]).toHaveAttribute("href", "https://files.test/export-1.csv");
    for (const download of downloads) {
      expect(download.className).toContain("focus-visible:outline-solid");
      expect(download.tabIndex).toBe(0);
    }

    // Tab lands on each history action in document order; Enter fires it.
    await tabTo(user, refresh);
    expect(refresh).toHaveFocus();
    await tabTo(user, next);
    expect(next).toHaveFocus();
    await pressEnter(user);
    await waitFor(() => expect(historyRows()).toHaveLength(1));
    expect(urls.some((url) => url.includes("cursor=10%3A1%3A0"))).toBe(true);

    await tabTo(user, prev);
    expect(prev).toHaveFocus();
    expect(prev).toBeEnabled();
    await pressEnter(user);
    await waitFor(() => expect(historyRows()).toHaveLength(10));

    // With no text field focused, content keys change neither the pager nor
    // the table; no screen-level shortcut fires.
    const before = `${screen.getByRole("table").textContent} ${document.activeElement?.tagName}`;
    await expectContentKeysInert(
      user,
      () => `${screen.getByRole("table").textContent} ${document.activeElement?.tagName}`
    );
    expect(`${screen.getByRole("table").textContent} ${document.activeElement?.tagName}`).toBe(before);
  });
});
