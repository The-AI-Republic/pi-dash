// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  type EmailFormValues,
} from "../emailConfig.js";
import { SMTP_SAVE_ERROR_LOG, SMTP_SAVE_TOAST_MESSAGE, SMTP_SAVE_TOAST_TITLE } from "../hooks.js";
import { adminKeys } from "../queries.js";
import { AdminEmailForm, SMTP_SAVE_LABEL, SMTP_SAVING_LABEL } from "./AdminEmailForm.js";

function smtpRow(key: string, value: string | null) {
  return {
    id: "00000000-0000-4000-8000-000000000002",
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

const BLANKED_VALUES: EmailFormValues = {
  host: "",
  port: "587",
  from: "",
  user: "",
  password: "",
  security: "tls",
};

const SAVED_VALUES: EmailFormValues = {
  host: "smtp.example.com",
  port: "587",
  from: "team@example.com",
  user: "mailer",
  password: "secret",
  security: "tls",
};

function rowsFor(values: EmailFormValues, enabled: string): unknown[] {
  return [
    smtpRow(SMTP_HOST_KEY, values.host),
    smtpRow(SMTP_PORT_KEY, values.port),
    smtpRow(SMTP_FROM_KEY, values.from),
    smtpRow(SMTP_USER_KEY, values.user),
    smtpRow(SMTP_PASSWORD_KEY, values.password),
    smtpRow(SMTP_TLS_KEY, values.security === "tls" ? "1" : "0"),
    smtpRow(SMTP_SSL_KEY, values.security === "ssl" ? "1" : "0"),
    smtpRow(SMTP_ENABLED_KEY, enabled),
  ];
}

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

function renderForm(initial: EmailFormValues, onSaved: () => void): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminEmailForm initial={initial} onSaved={onSaved} />
    </QueryClientProvider>
  );
  return queryClient;
}

function fill(field: string, value: string): void {
  fireEvent.change(screen.getByTestId(field), { target: { value } });
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("AdminEmailForm", () => {
  it("shuts both actions on a fresh invalid form", () => {
    createAppClient("https://api.test", stubTransport({}));
    renderForm(BLANKED_VALUES, vi.fn());
    expect(screen.getByTestId("admin-email-host")).toHaveValue("");
    expect(screen.getByTestId("admin-email-port")).toHaveValue("587");
    expect(screen.getByTestId("admin-email-save")).toBeDisabled();
    expect(screen.getByTestId("admin-email-save")).toHaveTextContent(SMTP_SAVE_LABEL);
    expect(screen.getByTestId("admin-email-test-email")).toBeDisabled();
    expect(screen.queryByTestId("admin-email-save-first-hint")).not.toBeInTheDocument();
  });

  it.each([
    ["invalid", BLANKED_VALUES],
    ["pristine", SAVED_VALUES],
  ])("submitting an %s form sends nothing", (_label, initial) => {
    const calls: string[] = [];
    createAppClient("https://api.test", ((url: string, options?: { method?: string }) => {
      calls.push(`${options?.method} ${new URL(url, "https://api.test").pathname}`);
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify([])),
      });
    }) as Transport);
    const onSaved = vi.fn();
    renderForm(initial, onSaved);
    fireEvent.submit(screen.getByTestId("admin-email-form"));
    expect(calls.filter((call) => call.startsWith("PATCH"))).toEqual([]);
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("opens save on valid+dirty and keeps the test action open too (NEWFRONT-477)", () => {
    createAppClient("https://api.test", stubTransport({}));
    renderForm(SAVED_VALUES, vi.fn());
    expect(screen.getByTestId("admin-email-save")).toBeDisabled();
    expect(screen.getByTestId("admin-email-test-email")).not.toBeDisabled();

    fill("admin-email-host", "smtp.example.com");
    expect(screen.getByTestId("admin-email-save")).toBeDisabled();
    fill("admin-email-host", "smtp2.example.com");
    expect(screen.getByTestId("admin-email-save")).not.toBeDisabled();
    // The check grades the entered values, so unsaved edits no longer
    // close the test action and there is no save-first hint.
    expect(screen.getByTestId("admin-email-test-email")).not.toBeDisabled();
  });

  it("shuts the test action the moment a required field goes blank", () => {
    createAppClient("https://api.test", stubTransport({}));
    renderForm(SAVED_VALUES, vi.fn());
    fill("admin-email-host", "");
    expect(screen.getByTestId("admin-email-test-email")).toBeDisabled();
  });

  it("saves valid+dirty values with SMTP forced on, then turns pristine", async () => {
    const calls: unknown[] = [];
    const answered = rowsFor({ ...SAVED_VALUES, host: "smtp2.example.com" }, "1");
    createAppClient("https://api.test", ((url: string, options?: { method?: string; body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (options?.method === "PATCH" && path === "/api/instances/configurations/") {
        calls.push(options.body === undefined ? undefined : JSON.parse(options.body));
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
    const onSaved = vi.fn();
    renderForm(SAVED_VALUES, onSaved);
    fill("admin-email-host", "smtp2.example.com");
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(calls).toEqual([
      {
        [SMTP_HOST_KEY]: "smtp2.example.com",
        [SMTP_PORT_KEY]: "587",
        [SMTP_FROM_KEY]: "team@example.com",
        [SMTP_USER_KEY]: "mailer",
        [SMTP_PASSWORD_KEY]: "secret",
        [SMTP_TLS_KEY]: "1",
        [SMTP_SSL_KEY]: "0",
        [SMTP_ENABLED_KEY]: "1",
      },
    ]);
    expect(notifySuccess).toHaveBeenCalledWith(SMTP_SAVE_TOAST_TITLE, SMTP_SAVE_TOAST_MESSAGE);
    // The save lands the form back to pristine: save shuts, the test
    // action opens, and the save-first hint clears.
    expect(screen.getByTestId("admin-email-save")).toBeDisabled();
    expect(screen.getByTestId("admin-email-test-email")).not.toBeDisabled();
    expect(screen.queryByTestId("admin-email-save-first-hint")).not.toBeInTheDocument();
  });

  it("patches every text field through to the save under its own key", async () => {
    const calls: unknown[] = [];
    const typed = {
      ...SAVED_VALUES,
      host: "smtp2.example.com",
      port: "2525",
      from: "alerts@example.com",
      user: "courier",
      password: "rotated",
    };
    const answered = rowsFor(typed, "1");
    createAppClient("https://api.test", ((url: string, options?: { method?: string; body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (options?.method === "PATCH" && path === "/api/instances/configurations/") {
        calls.push(options.body === undefined ? undefined : JSON.parse(options.body));
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
    const onSaved = vi.fn();
    renderForm(SAVED_VALUES, onSaved);
    fill("admin-email-host", "smtp2.example.com");
    fill("admin-email-port", "2525");
    fill("admin-email-from", "alerts@example.com");
    fill("admin-email-user", "courier");
    fill("admin-email-password", "rotated");
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(calls).toEqual([
      {
        [SMTP_HOST_KEY]: "smtp2.example.com",
        [SMTP_PORT_KEY]: "2525",
        [SMTP_FROM_KEY]: "alerts@example.com",
        [SMTP_USER_KEY]: "courier",
        [SMTP_PASSWORD_KEY]: "rotated",
        [SMTP_TLS_KEY]: "1",
        [SMTP_SSL_KEY]: "0",
        [SMTP_ENABLED_KEY]: "1",
      },
    ]);
  });

  it("swaps the save label while the write is in flight and ignores a second submit", async () => {
    const gate = deferred<{ status: number; url: string; headers: { get: () => null }; text: () => Promise<string> }>();
    let patches = 0;
    createAppClient("https://api.test", ((url: string, options?: { method?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (options?.method === "PATCH" && path === "/api/instances/configurations/") {
        patches += 1;
        return gate.promise;
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    const answered = rowsFor(SAVED_VALUES, "1");
    renderForm(SAVED_VALUES, vi.fn());
    fill("admin-email-host", "smtp2.example.com");
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(screen.getByTestId("admin-email-save")).toHaveTextContent(SMTP_SAVING_LABEL));
    expect(screen.getByTestId("admin-email-save")).toBeDisabled();
    fireEvent.submit(screen.getByTestId("admin-email-form"));
    gate.resolve({
      status: 200,
      url: "https://api.test/api/instances/configurations/",
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(answered)),
    });
    await waitFor(() => expect(screen.getByTestId("admin-email-save")).toHaveTextContent(SMTP_SAVE_LABEL));
    expect(patches).toBe(1);
  });

  it("logs a failed save to the console with no notice and stays dirty", async () => {
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/configurations/": { status: 500, body: { detail: "gone" } } })
    );
    const queryClient = renderForm(SAVED_VALUES, vi.fn());
    queryClient.setQueryData(adminKeys.configurations, rowsFor(SAVED_VALUES, "1"));
    fill("admin-email-host", "smtp2.example.com");
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(errors.length).toBe(1));
    expect(errors[0]?.[0]).toBe(SMTP_SAVE_ERROR_LOG);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
    expect(screen.getByTestId("admin-email-save")).toHaveTextContent(SMTP_SAVE_LABEL);
    expect(screen.getByTestId("admin-email-save")).not.toBeDisabled();
    // A failed save leaves a valid, dirty form — the test action stays open.
    expect(screen.getByTestId("admin-email-test-email")).not.toBeDisabled();
    expect(screen.queryByTestId("admin-email-save-first-hint")).not.toBeInTheDocument();
  });

  it("flips the password field between hidden and shown without touching the value", () => {
    createAppClient("https://api.test", stubTransport({}));
    renderForm(SAVED_VALUES, vi.fn());
    const field = screen.getByTestId("admin-email-password");
    expect(field).toHaveAttribute("type", "password");
    expect(field).toHaveValue("secret");
    fireEvent.click(screen.getByTestId("admin-email-toggle-password"));
    expect(screen.getByTestId("admin-email-password")).toHaveAttribute("type", "text");
    expect(screen.getByTestId("admin-email-password")).toHaveValue("secret");
    fireEvent.click(screen.getByTestId("admin-email-toggle-password"));
    expect(screen.getByTestId("admin-email-password")).toHaveAttribute("type", "password");
  });

  it("dirties the form on a security-only flip and persists the flags (NEWFRONT-478 intent)", async () => {
    const calls: unknown[] = [];
    const answered = rowsFor({ ...SAVED_VALUES, security: "ssl" }, "1");
    createAppClient("https://api.test", ((url: string, options?: { method?: string; body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (options?.method === "PATCH" && path === "/api/instances/configurations/") {
        calls.push(options.body === undefined ? undefined : JSON.parse(options.body));
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
    renderForm(SAVED_VALUES, vi.fn());
    expect(screen.getByTestId("admin-email-security-tls")).toBeChecked();
    fireEvent.click(screen.getByTestId("admin-email-security-ssl"));
    expect(screen.getByTestId("admin-email-security-ssl")).toBeChecked();
    expect(screen.getByTestId("admin-email-save")).not.toBeDisabled();
    fireEvent.click(screen.getByTestId("admin-email-security-none"));
    expect(screen.getByTestId("admin-email-save")).not.toBeDisabled();
    fireEvent.click(screen.getByTestId("admin-email-security-ssl"));
    fireEvent.click(screen.getByTestId("admin-email-save"));
    await waitFor(() => expect(screen.getByTestId("admin-email-save")).toBeDisabled());
    expect(calls).toEqual([
      expect.objectContaining({ [SMTP_TLS_KEY]: "0", [SMTP_SSL_KEY]: "1", [SMTP_ENABLED_KEY]: "1" }),
    ]);
  });

  it("opens the test modal from a pristine valid form and resets it on close", async () => {
    const bodies: unknown[] = [];
    createAppClient("https://api.test", ((url: string, options?: { body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/email-credentials-check/") {
        bodies.push(options?.body === undefined ? undefined : JSON.parse(options.body));
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ message: "Email successfully sent." })),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderForm(SAVED_VALUES, vi.fn());
    fireEvent.click(screen.getByTestId("admin-email-test-email"));
    expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "compose");
    fireEvent.change(screen.getByTestId("admin-test-email-receiver"), {
      target: { value: "admin@example.com" },
    });
    fireEvent.click(screen.getByTestId("admin-test-email-send"));
    await waitFor(() => expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "success"));
    // The saved form's values ride along with the receiver.
    expect(bodies).toEqual([
      {
        receiver_email: "admin@example.com",
        EMAIL_HOST: "smtp.example.com",
        EMAIL_PORT: "587",
        EMAIL_FROM: "team@example.com",
        EMAIL_HOST_USER: "mailer",
        EMAIL_HOST_PASSWORD: "secret",
        EMAIL_USE_TLS: "1",
        EMAIL_USE_SSL: "0",
      },
    ]);
    fireEvent.click(screen.getByTestId("admin-test-email-dismiss"));
    await waitFor(() => expect(screen.queryByTestId("admin-test-email-step")).not.toBeInTheDocument());
    fireEvent.click(screen.getByTestId("admin-email-test-email"));
    expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "compose");
    expect(screen.getByTestId("admin-test-email-receiver")).toHaveValue("");
  });

  it("tests unsaved edits: the check carries the entered values, not the saved ones", async () => {
    const bodies: unknown[] = [];
    createAppClient("https://api.test", ((url: string, options?: { body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/email-credentials-check/") {
        bodies.push(options?.body === undefined ? undefined : JSON.parse(options.body));
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ message: "Email successfully sent." })),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    renderForm(SAVED_VALUES, vi.fn());
    fill("admin-email-host", "entered.example.com");
    fill("admin-email-from", "entered@example.com");
    fill("admin-email-password", "");
    fireEvent.click(screen.getByRole("radio", { name: "SSL" }));
    fireEvent.click(screen.getByTestId("admin-email-test-email"));
    fireEvent.change(screen.getByTestId("admin-test-email-receiver"), {
      target: { value: "admin@example.com" },
    });
    fireEvent.click(screen.getByTestId("admin-test-email-send"));
    await waitFor(() => expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "success"));
    expect(bodies).toEqual([
      {
        receiver_email: "admin@example.com",
        EMAIL_HOST: "entered.example.com",
        EMAIL_PORT: "587",
        EMAIL_FROM: "entered@example.com",
        EMAIL_HOST_USER: "mailer",
        // A cleared password means "keep the stored one", so it is left out.
        EMAIL_USE_TLS: "0",
        EMAIL_USE_SSL: "1",
      },
    ]);
  });
});
