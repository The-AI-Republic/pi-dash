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
import { AdminSetupForm, resolveSetupPrefill, SETUP_POST_PATH, SETUP_TITLE } from "./AdminSetupForm.js";

const STRONG = "Parity-Setup-1!";

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
      <AdminSetupForm search={search} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("resolveSetupPrefill", () => {
  it("starts empty with telemetry on", () => {
    expect(resolveSetupPrefill({})).toEqual({
      firstName: "",
      lastName: "",
      email: "",
      company: "",
      telemetryChecked: true,
    });
  });

  it("prefills every echoed field verbatim", () => {
    expect(
      resolveSetupPrefill({
        first_name: "Parity",
        last_name: "Prefill",
        company: "Prefill Co",
        email: "a@example.com",
      })
    ).toEqual({
      firstName: "Parity",
      lastName: "Prefill",
      email: "a@example.com",
      company: "Prefill Co",
      telemetryChecked: true,
    });
  });

  it("prefers the server's company echo spelling (NEWFRONT-457)", () => {
    expect(resolveSetupPrefill({ company_name: "Server Co", company: "Crafted Co" }).company).toBe("Server Co");
    expect(resolveSetupPrefill({ company_name: "Server Co" }).company).toBe("Server Co");
    expect(resolveSetupPrefill({ company: "Crafted Co" }).company).toBe("Crafted Co");
  });

  it("turns telemetry off only on the explicit flag", () => {
    expect(resolveSetupPrefill({ is_telemetry_enabled: "False" }).telemetryChecked).toBe(false);
    expect(resolveSetupPrefill({ is_telemetry_enabled: "True" }).telemetryChecked).toBe(true);
    expect(resolveSetupPrefill({ is_telemetry_enabled: "0" }).telemetryChecked).toBe(true);
  });
});

describe("AdminSetupForm", () => {
  it("sets the document title and focuses the first name", () => {
    renderForm();
    expect(document.title).toBe(SETUP_TITLE);
    expect(screen.getByTestId("admin-setup-heading")).toHaveTextContent("Create the administrator account");
    expect(document.activeElement?.id).toBe("first_name");
  });

  it("posts natively with the hidden forgery token and telemetry flag", async () => {
    renderForm();
    const form = screen.getByTestId("admin-setup-form");
    expect(form).toHaveAttribute("method", "post");
    expect(form).toHaveAttribute("action", SETUP_POST_PATH);
    expect(SETUP_POST_PATH).toBe("/api/instances/admins/sign-up/");
    const token = form.querySelector('input[name="csrfmiddlewaretoken"]');
    expect(token).toHaveAttribute("value", "");
    await waitFor(() => expect(token).toHaveAttribute("value", "csrf-1"));
    const telemetry = form.querySelector('input[name="is_telemetry_enabled"]');
    expect(telemetry).toHaveAttribute("value", "True");
    fireEvent.click(screen.getByRole("checkbox"));
    expect(telemetry).toHaveAttribute("value", "False");
  });

  it("keeps a fresh form shut with a messageless meter and no mismatch", () => {
    renderForm();
    expect(screen.getByTestId("admin-setup-submit")).toBeDisabled();
    expect(screen.getByTestId("admin-setup-strength")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-setup-strength-message")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-setup-mismatch")).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox")).toBeChecked();
  });

  it("stays shut while the forgery token is still loading", () => {
    renderForm({}, null);
    fireEvent.change(screen.getByLabelText("First name"), { target: { value: "Parity" } });
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "a@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: STRONG } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: STRONG } });
    expect(screen.getByTestId("admin-setup-submit")).toBeDisabled();
  });

  it("gates on a strong secret with a live message", async () => {
    renderForm();
    fireEvent.change(screen.getByLabelText("First name"), { target: { value: "Parity" } });
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "a@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "short" } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "short" } });
    expect(screen.getByTestId("admin-setup-submit")).toBeDisabled();
    expect(screen.getByTestId("admin-setup-strength-message")).toHaveTextContent(/8 or more characters/);
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: STRONG } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: STRONG } });
    await waitFor(() => expect(screen.getByTestId("admin-setup-submit")).toBeEnabled());
  });

  it("shows the mismatch only once the confirmation is left or catches up", () => {
    renderForm();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: STRONG } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "Parity-Se" } });
    fireEvent.focus(screen.getByLabelText("Confirm password"));
    expect(screen.queryByTestId("admin-setup-mismatch")).not.toBeInTheDocument();
    fireEvent.blur(screen.getByLabelText("Confirm password"));
    expect(screen.getByTestId("admin-setup-mismatch")).toHaveTextContent("The passwords do not match.");
    expect(screen.getByTestId("admin-setup-submit")).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: STRONG } });
    expect(screen.queryByTestId("admin-setup-mismatch")).not.toBeInTheDocument();
  });

  it("hides the meter on blur with a strong secret and shows it on focus", () => {
    renderForm();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: STRONG } });
    fireEvent.blur(screen.getByLabelText("Password"));
    expect(screen.queryByTestId("admin-setup-strength")).not.toBeInTheDocument();
    fireEvent.focus(screen.getByLabelText("Password"));
    expect(screen.getByTestId("admin-setup-strength")).toBeInTheDocument();
    expect(screen.getByTestId("admin-setup-strength-message")).toHaveTextContent("Strong password.");
  });

  it("flips each password field independently with toggles out of tab order", () => {
    renderForm();
    const passwordToggle = screen.getByTestId("admin-setup-toggle-password");
    const confirmToggle = screen.getByTestId("admin-setup-toggle-confirm");
    expect(passwordToggle).toHaveAttribute("tabindex", "-1");
    expect(confirmToggle).toHaveAttribute("tabindex", "-1");
    const password = screen.getByLabelText("Password");
    const confirm = screen.getByLabelText("Confirm password");
    fireEvent.click(passwordToggle);
    expect(password).toHaveAttribute("type", "text");
    expect(confirm).toHaveAttribute("type", "password");
    expect(passwordToggle).toHaveAttribute("aria-label", "Hide password");
    fireEvent.click(confirmToggle);
    expect(confirm).toHaveAttribute("type", "text");
    fireEvent.click(passwordToggle);
    expect(password).toHaveAttribute("type", "password");
    expect(passwordToggle).toHaveAttribute("aria-label", "Show password");
  });

  it("keeps the toggles pressable while the strength meter collapses on blur", () => {
    // A strong secret hides the meter on blur, so a mousedown that moved
    // focus would reflow the form between press and release and swallow
    // the toggle. Both toggles cancel mousedown, so focus never moves.
    renderForm();
    const password = screen.getByLabelText("Password");
    const passwordToggle = screen.getByTestId("admin-setup-toggle-password");
    const confirmToggle = screen.getByTestId("admin-setup-toggle-confirm");
    fireEvent.change(password, { target: { value: STRONG } });
    fireEvent.focus(password);
    expect(screen.getByTestId("admin-setup-strength")).toBeInTheDocument();

    expect(fireEvent.mouseDown(passwordToggle)).toBe(false);
    expect(screen.getByTestId("admin-setup-strength")).toBeInTheDocument();
    fireEvent.click(passwordToggle);
    expect(password).toHaveAttribute("type", "text");

    const confirm = screen.getByLabelText("Confirm password");
    fireEvent.change(confirm, { target: { value: STRONG } });
    fireEvent.focus(confirm);
    expect(fireEvent.mouseDown(confirmToggle)).toBe(false);
    fireEvent.click(confirmToggle);
    expect(confirm).toHaveAttribute("type", "text");
  });

  it("swallows rejected names and caps lengths", () => {
    renderForm();
    const firstName = screen.getByLabelText("First name");
    fireEvent.change(firstName, { target: { value: "Parity" } });
    fireEvent.change(firstName, { target: { value: "Ann1" } });
    expect(firstName).toHaveValue("Parity");
    fireEvent.change(firstName, { target: { value: " " } });
    expect(firstName).toHaveValue("Parity");
    fireEvent.change(firstName, { target: { value: "Jean-Paul" } });
    expect(firstName).toHaveValue("Jean-Paul");
    fireEvent.change(firstName, { target: { value: "a".repeat(60) } });
    expect(firstName).toHaveValue("Jean-Paul");
    expect(firstName).toHaveAttribute("maxlength", "50");
    const lastName = screen.getByLabelText("Last name");
    fireEvent.change(lastName, { target: { value: "Admin" } });
    fireEvent.change(lastName, { target: { value: "Adm1n" } });
    expect(lastName).toHaveValue("Admin");
    const company = screen.getByLabelText("Company");
    fireEvent.change(company, { target: { value: "Acme{Corp}" } });
    expect(company).toHaveValue("");
    fireEvent.change(company, { target: { value: "Acme_123 Corp-x" } });
    expect(company).toHaveValue("Acme_123 Corp-x");
    fireEvent.change(company, { target: { value: "b".repeat(90) } });
    expect(company).toHaveValue("Acme_123 Corp-x");
    expect(company).toHaveAttribute("maxlength", "80");
  });

  it("prefills from the redirect params with telemetry defaulting on", () => {
    renderForm({ first_name: "Parity", last_name: "Prefill", company: "Prefill Co", email: "a@example.com" });
    expect(screen.getByLabelText("First name")).toHaveValue("Parity");
    expect(screen.getByLabelText("Last name")).toHaveValue("Prefill");
    expect(screen.getByLabelText("Email")).toHaveValue("a@example.com");
    expect(screen.getByLabelText("Company")).toHaveValue("Prefill Co");
    expect(screen.getByRole("checkbox")).toBeChecked();
  });

  it("prefills the company from the server echo spelling and honors the telemetry flag", () => {
    renderForm({ company_name: "Server Co", is_telemetry_enabled: "False" });
    expect(screen.getByLabelText("Company")).toHaveValue("Server Co");
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });

  it("renders crafted email/password codes inline under their field", () => {
    const { unmount } = renderForm({ error_code: "INVALID_EMAIL", error_message: "parity-probe-email" });
    expect(screen.getByText("parity-probe-email")).toHaveAttribute("id", "email-error");
    expect(screen.queryByTestId("admin-setup-banner")).not.toBeInTheDocument();
    unmount();

    renderForm({ error_code: "INVALID_PASSWORD", error_message: "parity-probe-password" });
    expect(screen.getByText("parity-probe-password")).toHaveAttribute("id", "password-error");
    expect(screen.queryByTestId("admin-setup-banner")).not.toBeInTheDocument();
  });

  it("renders crafted banner codes on top and unknown codes silently", () => {
    const { unmount } = renderForm({ error_code: "ADMIN_ALREADY_EXIST", error_message: "parity-probe-banner" });
    expect(screen.getByTestId("admin-setup-banner")).toHaveTextContent("parity-probe-banner");
    unmount();

    renderForm({ error_code: "NO_SUCH_CODE", error_message: "parity-probe-unknown" });
    expect(screen.queryByTestId("admin-setup-banner")).not.toBeInTheDocument();
    expect(screen.queryByText("parity-probe-unknown")).not.toBeInTheDocument();
  });

  it("renders numeric failures like crafted codes (NEWFRONT-457)", () => {
    renderForm({ error_code: "5180", error_message: "taken", email: "taken@example.com", company_name: "Bug Co" });
    expect(screen.getByTestId("admin-setup-banner")).toHaveTextContent("taken");
    expect(screen.getByLabelText("Email")).toHaveValue("taken@example.com");
    expect(screen.getByLabelText("Company")).toHaveValue("Bug Co");
  });

  it("shows the busy state while the native POST is in flight", async () => {
    renderForm();
    fireEvent.change(screen.getByLabelText("First name"), { target: { value: "Parity" } });
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "a@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: STRONG } });
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: STRONG } });
    const submit = screen.getByTestId("admin-setup-submit");
    await waitFor(() => expect(submit).toBeEnabled());
    fireEvent.submit(screen.getByTestId("admin-setup-form"));
    expect(submit).toHaveAttribute("aria-busy", "true");
    expect(submit).toBeDisabled();
  });
});
