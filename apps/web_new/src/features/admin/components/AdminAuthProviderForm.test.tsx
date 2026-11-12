// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider form: credential inputs over the draft with password
// show/hide, the staged sync switch, copy-out URLs, save gating with
// the busy label and the echo re-read, silent required blocking,
// log-only failure, and the discard guard on dirty back.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import type { ConfigMap } from "../authMethods.js";
import { AUTH_PROVIDER_BY_SLUG, type AuthProviderDef, type ProviderCredentialField } from "../providerAuth.js";
import { AdminAuthProviderForm } from "./AdminAuthProviderForm.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifySuccess: vi.fn() };
});

const mockedNotifySuccess = vi.mocked(notifySuccess);

function configRow(key: string, value: string | null) {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "AUTHENTICATION",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

const FIELDS: readonly ProviderCredentialField[] = [
  { key: "CLIENT_ID", label: "Client ID", type: "text", required: true, placeholder: "sample-id" },
  { key: "CLIENT_SECRET", label: "Client secret", type: "password", required: true },
  {
    key: "ORG_ID",
    label: "Organization ID",
    type: "text",
    required: false,
    description: "Optional organization scope.",
  },
];

const CONFIG: ConfigMap = {
  CLIENT_ID: "id-1",
  CLIENT_SECRET: "secret-1",
  ORG_ID: "",
  ENABLE_GITHUB_SYNC: "0",
};

interface FormStub {
  bodies: unknown[];
  release: () => void;
  transport: Transport;
}

function stubFormTransport(args: {
  patchStatus?: number;
  hangPatch?: boolean;
  patchEcho?: Record<string, string>;
}): FormStub {
  const bodies: unknown[] = [];
  let release = () => {};
  const gate = args.hangPatch === true ? new Promise<void>((resolve) => (release = resolve)) : null;
  const transport = (async (url: string, init?: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf" })),
      };
    }
    const method = init?.method ?? "GET";
    const path = new URL(url, "https://api.test").pathname;
    if (path === "/api/instances/configurations/" && method === "PATCH") {
      const parsed = (init?.body === undefined ? {} : JSON.parse(init.body)) as Record<string, string>;
      bodies.push(parsed);
      if (gate !== null) await gate;
      if (args.patchStatus === 500) {
        return {
          status: 500,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
        };
      }
      const echoed = args.patchEcho ?? parsed;
      const answered = Object.entries(echoed).map(([key, value]) => configRow(key, value));
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(answered)),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: () => Promise.resolve("{}") };
  }) as Transport;
  return { bodies, release, transport };
}

function renderForm(
  provider: AuthProviderDef,
  config: ConfigMap,
  extra?: {
    fields?: readonly ProviderCredentialField[];
    prefill?: (key: string, stored: string) => string;
    copyDescriptions?: Partial<Record<string, React.ReactNode>>;
    copyHelp?: Partial<Record<string, { href: string | ((values: Record<string, string>) => string); label: string }>>;
  }
): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminAuthProviderForm
        provider={provider}
        config={config}
        {...(extra?.fields === undefined ? {} : { fields: extra.fields })}
        {...(extra?.prefill === undefined ? {} : { prefill: extra.prefill })}
        {...(extra?.copyDescriptions === undefined ? {} : { copyDescriptions: extra.copyDescriptions })}
        {...(extra?.copyHelp === undefined ? {} : { copyHelp: extra.copyHelp })}
      />
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

describe("provider form", () => {
  it("renders credential inputs, the sync switch, copy fields, and the save/back row", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    const idInput = screen.getByTestId("admin-auth-field-CLIENT_ID");
    expect(idInput).toHaveAttribute("id", "CLIENT_ID");
    expect(idInput).toHaveAttribute("type", "text");
    expect(idInput).toHaveAttribute("placeholder", "sample-id");
    expect(idInput).toHaveValue("id-1");
    expect(screen.getByTestId("admin-auth-field-CLIENT_SECRET")).toHaveAttribute("type", "password");
    expect(screen.getByText("Optional organization scope.")).toBeInTheDocument();

    expect(screen.getByText("Refresh user attributes from GitHub during sign in")).toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-provider-sync")).toHaveAttribute("aria-checked", "false");

    expect(screen.getByText("https://api.example.test")).toBeInTheDocument();
    expect(screen.getByText("https://api.example.test/auth/github/callback/")).toBeInTheDocument();

    const save = screen.getByTestId("admin-auth-provider-save");
    expect(save).toBeDisabled();
    expect(save).toHaveTextContent("Save changes");
    expect(screen.getByTestId("admin-auth-provider-back")).toHaveAttribute("href", "/god-mode/authentication/");
  });

  it("renders the shared controls without credential inputs by default", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.gitlab, { ENABLE_GITLAB_SYNC: "1" });

    expect(screen.queryByTestId("admin-auth-field-CLIENT_ID")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-provider-sync")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("https://api.example.test/auth/gitlab/callback/")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-auth-copy-origin-url")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-provider-save")).toBeDisabled();
  });

  it("reveals and hides password fields without touching text fields", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    expect(screen.queryByRole("button", { name: "Show Client ID" })).not.toBeInTheDocument();
    const show = screen.getByRole("button", { name: "Show Client secret" });
    fireEvent.click(show);
    expect(screen.getByTestId("admin-auth-field-CLIENT_SECRET")).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "Hide Client secret" }));
    expect(screen.getByTestId("admin-auth-field-CLIENT_SECRET")).toHaveAttribute("type", "password");
  });

  it("prefills stored values per key", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(
      AUTH_PROVIDER_BY_SLUG.gitea,
      { GITEA_HOST: "" },
      {
        fields: [{ key: "GITEA_HOST", label: "Host", type: "text", required: true }],
        prefill: (key, stored) => (stored === "" ? `default-for-${key}` : stored),
      }
    );
    expect(screen.getByTestId("admin-auth-field-GITEA_HOST")).toHaveValue("default-for-GITEA_HOST");
  });

  it("gates the save on dirtiness both ways", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    const save = screen.getByTestId("admin-auth-provider-save");
    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-2" } });
    expect(save).not.toBeDisabled();
    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-1" } });
    expect(save).toBeDisabled();
  });

  it("stages the sync flip without a request, then persists everything with Done", async () => {
    const stub = stubFormTransport({});
    createAppClient("https://api.example.test", stub.transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.click(screen.getByTestId("admin-auth-provider-sync"));
    expect(screen.getByTestId("admin-auth-provider-sync")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-auth-provider-save")).not.toBeDisabled();
    expect(stub.bodies).toEqual([]);

    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-2" } });
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith(
        "Done!",
        "Your GitHub authentication is configured. You should test it now."
      )
    );
    expect(stub.bodies).toEqual([
      { CLIENT_ID: "id-2", CLIENT_SECRET: "secret-1", ORG_ID: "", ENABLE_GITHUB_SYNC: "1" },
    ]);
    expect(screen.getByTestId("admin-auth-provider-save")).toBeDisabled();
  });

  it("re-reads the draft from the server echo on success", async () => {
    const stub = stubFormTransport({
      patchEcho: { CLIENT_ID: "echo-id", CLIENT_SECRET: "echo-secret", ORG_ID: "", ENABLE_GITHUB_SYNC: "1" },
    });
    createAppClient("https://api.example.test", stub.transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.click(screen.getByTestId("admin-auth-provider-sync"));
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    await waitFor(() => expect(mockedNotifySuccess).toHaveBeenCalled());
    expect(screen.getByTestId("admin-auth-field-CLIENT_ID")).toHaveValue("echo-id");
    expect(screen.getByTestId("admin-auth-provider-sync")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-auth-provider-save")).toBeDisabled();
  });

  it("shows the busy label in flight and sends one request for a double submit", async () => {
    const stub = stubFormTransport({ hangPatch: true });
    createAppClient("https://api.example.test", stub.transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.click(screen.getByTestId("admin-auth-provider-sync"));
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-save")).toHaveTextContent("Saving"));
    expect(screen.getByTestId("admin-auth-provider-form")).toHaveAttribute("aria-busy", "true");
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    stub.release();
    await waitFor(() => expect(mockedNotifySuccess).toHaveBeenCalled());
    expect(stub.bodies).toHaveLength(1);
    expect(screen.getByTestId("admin-auth-provider-save")).toHaveTextContent("Save changes");
  });

  it("blocks an unfinished required field silently", () => {
    const stub = stubFormTransport({});
    createAppClient("https://api.example.test", stub.transport);
    renderForm(
      AUTH_PROVIDER_BY_SLUG.github,
      { CLIENT_ID: "", CLIENT_SECRET: "", ENABLE_GITHUB_SYNC: "0" },
      {
        fields: FIELDS,
      }
    );

    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-9" } });
    expect(screen.getByTestId("admin-auth-provider-save")).not.toBeDisabled();
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    expect(stub.bodies).toEqual([]);
    expect(mockedNotifySuccess).not.toHaveBeenCalled();
    expect(screen.getByTestId("admin-auth-provider-save")).not.toBeDisabled();
  });

  it("logs a failed save with no notice and the form still dirty", async () => {
    const stub = stubFormTransport({ patchStatus: 500 });
    createAppClient("https://api.example.test", stub.transport);
    const errored = vi.spyOn(console, "error").mockImplementation(() => {});
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-2" } });
    fireEvent.click(screen.getByTestId("admin-auth-provider-save"));
    await waitFor(() => expect(errored).toHaveBeenCalled());
    expect(
      errored.mock.calls.some((call) => String(call[0]).includes("Error updating the instance configurations"))
    ).toBe(true);
    expect(stub.bodies).toHaveLength(1);
    expect(mockedNotifySuccess).not.toHaveBeenCalled();
    expect(screen.getByTestId("admin-auth-provider-save")).not.toBeDisabled();
    expect(screen.getByTestId("admin-auth-provider-save")).toHaveTextContent("Save changes");
  });

  it("renders copy descriptions and help links, deriving live targets from the draft", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, {
      fields: FIELDS,
      copyDescriptions: { "Origin URL": "Paste into the authorized origin field." },
      copyHelp: {
        "Origin URL": { href: "https://console.example.test/new", label: "Open the console" },
        "Callback URI": {
          href: (values) => `https://console.example.test/${values["CLIENT_ID"]}`,
          label: "Open this client",
        },
      },
    });

    expect(screen.getByText("Paste into the authorized origin field.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open the console" })).toHaveAttribute(
      "href",
      "https://console.example.test/new"
    );
    expect(screen.getByRole("link", { name: "Open this client" })).toHaveAttribute(
      "href",
      "https://console.example.test/id-1"
    );
    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-live" } });
    expect(screen.getByRole("link", { name: "Open this client" })).toHaveAttribute(
      "href",
      "https://console.example.test/id-live"
    );
  });

  it("backs straight out of a clean form with no modal", () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.click(screen.getByTestId("admin-auth-provider-back"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("guards a dirty back with the discard modal that keeps the draft", async () => {
    createAppClient("https://api.example.test", stubFormTransport({}).transport);
    renderForm(AUTH_PROVIDER_BY_SLUG.github, CONFIG, { fields: FIELDS });

    fireEvent.change(screen.getByTestId("admin-auth-field-CLIENT_ID"), { target: { value: "id-dirty" } });
    fireEvent.click(screen.getByTestId("admin-auth-provider-back"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-discard-leave")).toHaveAttribute("href", "/god-mode/authentication/");

    fireEvent.click(screen.getByTestId("admin-auth-discard-keep"));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByTestId("admin-auth-field-CLIENT_ID")).toHaveValue("id-dirty");
  });
});
