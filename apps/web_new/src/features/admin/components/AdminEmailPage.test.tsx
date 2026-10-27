// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  SMTP_ENABLED_KEY,
  SMTP_FROM_KEY,
  SMTP_HOST_KEY,
  SMTP_PASSWORD_KEY,
  SMTP_PORT_KEY,
  SMTP_SSL_KEY,
  SMTP_TLS_KEY,
  SMTP_USER_KEY,
} from "../emailConfig.js";
import {
  EMAIL_DISABLE_ERROR_LOG,
  EMAIL_DISABLE_FAILED_MESSAGE,
  EMAIL_DISABLE_FAILED_TITLE,
  EMAIL_DISABLED_TOAST_MESSAGE,
  EMAIL_DISABLED_TOAST_TITLE,
} from "../hooks.js";
import { AdminEmailPage, EMAIL_PAGE_TITLE } from "./AdminEmailPage.js";

function smtpRow(key: string, value: string | null) {
  return {
    id: "00000000-0000-4000-8000-000000000003",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "smtp",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

const ENABLED_ROWS = [
  smtpRow(SMTP_HOST_KEY, "smtp.example.com"),
  smtpRow(SMTP_PORT_KEY, "587"),
  smtpRow(SMTP_FROM_KEY, "team@example.com"),
  smtpRow(SMTP_USER_KEY, "mailer"),
  smtpRow(SMTP_PASSWORD_KEY, "secret"),
  smtpRow(SMTP_TLS_KEY, "1"),
  smtpRow(SMTP_SSL_KEY, "0"),
  smtpRow(SMTP_ENABLED_KEY, "1"),
];

const DISABLED_ROWS = [
  smtpRow(SMTP_HOST_KEY, ""),
  smtpRow(SMTP_PORT_KEY, "587"),
  smtpRow(SMTP_FROM_KEY, ""),
  smtpRow(SMTP_USER_KEY, ""),
  smtpRow(SMTP_PASSWORD_KEY, ""),
  smtpRow(SMTP_TLS_KEY, "1"),
  smtpRow(SMTP_SSL_KEY, "0"),
  smtpRow(SMTP_ENABLED_KEY, "0"),
];

function stubTransport(routes: Record<string, { status: number; body: unknown }>): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function renderPage(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminEmailPage />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("AdminEmailPage", () => {
  it("shows only the header pill skeleton while configurations load", async () => {
    const gate = deferred<{ status: number; url: string; headers: { get: () => null }; text: () => Promise<string> }>();
    createAppClient("https://api.test", ((url: string) => {
      if (new URL(url, "https://api.test").pathname === "/api/instances/configurations/") {
        return gate.promise;
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderPage();
    expect(screen.getByTestId("admin-email-header-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-form-skeleton")).not.toBeInTheDocument();
    gate.resolve({
      status: 200,
      url: "https://api.test/api/instances/configurations/",
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(DISABLED_ROWS)),
    });
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-email-header-skeleton")).not.toBeInTheDocument();
  });

  it("reads a failed load as off and skeletons the form when toggled on with no configuration", async () => {
    // Toggling back off fires a disable that fails against the stub; the
    // error notice is covered elsewhere, so keep the expected log quiet.
    vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient("https://api.test", stubTransport({}));
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toBeInTheDocument());
    expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-header-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-form-skeleton")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    const skeleton = await screen.findByTestId("admin-email-form-skeleton");
    expect(within(skeleton).getAllByTestId("admin-email-form-skeleton-row")).toHaveLength(5);
    expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-email-header-skeleton")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(screen.queryByTestId("admin-email-form-skeleton")).not.toBeInTheDocument());
  });

  it("disables through the endpoint once, then re-enables locally with no call", async () => {
    const calls: string[] = [];
    createAppClient("https://api.test", ((url: string, options?: { method?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      calls.push(`${options?.method} ${path}`);
      if (path === "/api/instances/configurations/") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(ENABLED_ROWS)),
        });
      }
      if (path === "/api/instances/configurations/disable-email-feature/") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(""),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "true"));
    expect(screen.getByTestId("admin-email-form")).toBeInTheDocument();
    expect(screen.getByTestId("admin-email-host")).toHaveValue("smtp.example.com");

    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument());
    expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "false");
    expect(calls.filter((call) => call === "DELETE /api/instances/configurations/disable-email-feature/")).toHaveLength(
      1
    );
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(notifySuccess).toHaveBeenCalledWith(EMAIL_DISABLED_TOAST_TITLE, EMAIL_DISABLED_TOAST_MESSAGE);

    const settled = calls.length;
    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(screen.getByTestId("admin-email-form")).toBeInTheDocument());
    expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-email-host")).toHaveValue("");
    await sleep(100);
    expect(calls.length).toBe(settled);
  });

  it("keeps the toggle on with the form visible when the disable fails (NEWFRONT-476 intent)", async () => {
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/configurations/": { status: 200, body: ENABLED_ROWS },
        "/api/instances/configurations/disable-email-feature/": {
          status: 400,
          body: { error: "Failed to disable email configuration" },
        },
      })
    );
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "true"));

    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(EMAIL_DISABLE_FAILED_TITLE, EMAIL_DISABLE_FAILED_MESSAGE);
    expect(notifySuccess).not.toHaveBeenCalled();
    // The parity scenario polls the console for this line through the
    // real toggle path, so the page pins it too, not just the hook test.
    expect(errors.some((args) => args[0] === EMAIL_DISABLE_ERROR_LOG)).toBe(true);
    expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-email-form")).toBeInTheDocument();
    expect(screen.getByTestId("admin-email-host")).toHaveValue("smtp.example.com");
  });

  it("shuts the toggle off while the disable is in flight", async () => {
    const gate = deferred<{ status: number; url: string; headers: { get: () => null }; text: () => Promise<string> }>();
    createAppClient("https://api.test", ((url: string) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/configurations/") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(ENABLED_ROWS)),
        });
      }
      if (path === "/api/instances/configurations/disable-email-feature/") return gate.promise;
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "true"));
    expect(screen.getByTestId("admin-email-toggle")).not.toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-disabled", "true"));
    gate.resolve({
      status: 200,
      url: "https://api.test/api/instances/configurations/disable-email-feature/",
      headers: { get: () => null },
      text: () => Promise.resolve(""),
    });
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).not.toHaveAttribute("aria-disabled", "true"));
  });

  it("reveals the stored values from a disabled start and hides again on save without enable", async () => {
    createAppClient("https://api.test", ((url: string, options?: { method?: string; body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/configurations/" && options?.method !== "PATCH") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(DISABLED_ROWS)),
        });
      }
      if (path === "/api/instances/configurations/" && options?.method === "PATCH") {
        // The server answers the mail keys but has no enable-flag row, so
        // the stored flag stays down: the toggle must follow the store.
        const answered = [
          smtpRow(SMTP_HOST_KEY, "smtp.example.com"),
          smtpRow(SMTP_PORT_KEY, "587"),
          smtpRow(SMTP_FROM_KEY, "team@example.com"),
          smtpRow(SMTP_USER_KEY, ""),
          smtpRow(SMTP_PASSWORD_KEY, ""),
          smtpRow(SMTP_TLS_KEY, "1"),
          smtpRow(SMTP_SSL_KEY, "0"),
        ];
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(answered)),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toBeInTheDocument());
    expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("admin-email-toggle"));
    await waitFor(() => expect(screen.getByTestId("admin-email-form")).toBeInTheDocument());
    expect(screen.getByTestId("admin-email-host")).toHaveValue("");
    expect(screen.getByTestId("admin-email-port")).toHaveValue("587");

    fireEvent.change(screen.getByTestId("admin-email-host"), { target: { value: "smtp.example.com" } });
    fireEvent.change(screen.getByTestId("admin-email-from"), { target: { value: "team@example.com" } });
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toHaveAttribute("aria-checked", "false"));
    expect(screen.queryByTestId("admin-email-form")).not.toBeInTheDocument();
  });

  it("sets the email document title", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/configurations/": { status: 200, body: DISABLED_ROWS } })
    );
    renderPage();
    await waitFor(() => expect(screen.getByTestId("admin-email-toggle")).toBeInTheDocument());
    expect(document.title).toBe(EMAIL_PAGE_TITLE);
    expect(screen.getByTestId("admin-email-heading")).toHaveTextContent("Email");
  });
});
