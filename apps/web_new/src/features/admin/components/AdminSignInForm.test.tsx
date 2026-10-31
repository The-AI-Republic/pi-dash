// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { AdminEntrySearch } from "./AdminEntryGate.js";
import { AdminSignInForm, SIGN_IN_POST_PATH, SIGN_IN_TITLE } from "./AdminSignInForm.js";

function csrfTransport(token: string | null): Transport {
  return (url) => {
    if (new URL(url, "https://api.test").pathname === "/auth/get-csrf-token/" && token !== null) {
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: token })),
      });
    }
    return new Promise(() => {});
  };
}

function renderForm(search: AdminEntrySearch = {}, token: string | null = "csrf-1") {
  createAppClient("https://api.test", csrfTransport(token));
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminSignInForm search={search} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("AdminSignInForm", () => {
  it("sets the title, focuses the email, and starts shut with no banners", () => {
    renderForm();
    expect(document.title).toBe(SIGN_IN_TITLE);
    expect(screen.getByTestId("admin-sign-in-heading")).toHaveTextContent("Sign in to instance admin");
    expect(document.activeElement?.id).toBe("email");
    expect(screen.getByTestId("admin-sign-in-submit")).toBeDisabled();
    expect(screen.queryByTestId("admin-sign-in-error")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-sign-in-notice")).not.toBeInTheDocument();
  });

  it("posts natively with the hidden forgery token", async () => {
    renderForm();
    const form = screen.getByTestId("admin-sign-in-form");
    expect(form).toHaveAttribute("method", "post");
    expect(form).toHaveAttribute("action", SIGN_IN_POST_PATH);
    expect(SIGN_IN_POST_PATH).toBe("/api/instances/admins/sign-in/");
    const token = form.querySelector('input[name="csrfmiddlewaretoken"]');
    await waitFor(() => expect(token).toHaveAttribute("value", "csrf-1"));
  });

  it("prefills the email from the redirect param", () => {
    renderForm({ email: "admin@example.com" });
    expect(screen.getByLabelText("Email")).toHaveValue("admin@example.com");
  });

  it("opens submit once both fields are non-empty", async () => {
    renderForm();
    const submit = screen.getByTestId("admin-sign-in-submit");
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "admin@example.com" } });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    await waitFor(() => expect(submit).toBeEnabled());
  });

  it("stays shut while the forgery token is still loading", () => {
    renderForm({}, null);
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "admin@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    expect(screen.getByTestId("admin-sign-in-submit")).toBeDisabled();
  });

  it("flips the password field with its toggle", () => {
    renderForm();
    const toggle = screen.getByTestId("admin-sign-in-toggle-password");
    const password = screen.getByLabelText("Password");
    expect(toggle).toHaveAttribute("tabindex", "-1");
    expect(password).toHaveAttribute("type", "password");
    fireEvent.click(toggle);
    expect(password).toHaveAttribute("type", "text");
    expect(toggle).toHaveAttribute("aria-label", "Hide password");
    fireEvent.click(toggle);
    expect(password).toHaveAttribute("type", "password");
    expect(toggle).toHaveAttribute("aria-label", "Show password");
  });

  it("shows the busy state while the native POST is in flight", async () => {
    renderForm();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "admin@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    const submit = screen.getByTestId("admin-sign-in-submit");
    await waitFor(() => expect(submit).toBeEnabled());
    fireEvent.submit(screen.getByTestId("admin-sign-in-form"));
    expect(submit).toHaveAttribute("aria-busy", "true");
    expect(submit).toBeDisabled();
  });

  it("renders a crafted known code as the generic banner", () => {
    renderForm({ error_code: "INVALID_EMAIL", error_message: "parity-probe-generic" });
    expect(screen.getByTestId("admin-sign-in-error")).toHaveTextContent("parity-probe-generic");
    expect(screen.queryByTestId("admin-sign-in-notice")).not.toBeInTheDocument();
  });

  it("renders a crafted unknown code with no banner", () => {
    renderForm({ error_code: "NO_SUCH_CODE", error_message: "parity-probe-unknown" });
    expect(screen.queryByTestId("admin-sign-in-error")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-sign-in-notice")).not.toBeInTheDocument();
  });

  it("maps a wrong-secret failure to a dismissible banner naming the outcome", () => {
    renderForm({ error_code: "5175", error_message: "ADMIN_AUTHENTICATION_FAILED" });
    expect(screen.queryByTestId("admin-sign-in-error")).not.toBeInTheDocument();
    const notice = screen.getByTestId("admin-sign-in-notice");
    expect(notice).toHaveTextContent("Authentication failed");
    expect(notice.querySelector("a")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss notice" }));
    expect(screen.queryByTestId("admin-sign-in-notice")).not.toBeInTheDocument();
  });

  it("links the unknown-account banner back to the sign-in entry (NEWFRONT-458)", () => {
    renderForm({ error_code: "5185", error_message: "ADMIN_USER_DOES_NOT_EXIST" });
    const notice = screen.getByTestId("admin-sign-in-notice");
    expect(notice.querySelector("a")).toHaveAttribute("href", "/god-mode/");
  });
});
