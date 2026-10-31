// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider credential field sets: keys, types, required flags, and host
// placeholders per provider; the Gitea public-host prefill; the live
// application-settings link (the NEWFRONT-492 intended behavior); and
// the copy-field paste guidance with console help links.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { resolveCopyHelpHref, AUTH_PROVIDER_BY_SLUG } from "./providerAuth.js";
import {
  GITEA_HOST_KEY,
  GITEA_PUBLIC_APPLICATIONS_URL,
  GITEA_PUBLIC_HOST,
  GITLAB_OAUTH_DOCS_URL,
  GITLAB_PUBLIC_HOST,
  GITHUB_OAUTH_CONSOLE_URL,
  GOOGLE_CLIENT_GUIDE_URL,
  GOOGLE_OAUTH_CONSOLE_URL,
  GOOGLE_SECRET_GUIDE_URL,
  PROVIDER_CREDENTIALS,
  giteaApplicationsHref,
  giteaHostPrefill,
} from "./providerCredentials.js";
import { AdminAuthCopyField } from "./components/AdminAuthCopyField.js";
import { AdminAuthProviderForm } from "./components/AdminAuthProviderForm.js";

describe("giteaHostPrefill", () => {
  it("defaults a blank stored host to the public service", () => {
    expect(giteaHostPrefill(GITEA_HOST_KEY, "")).toBe(GITEA_PUBLIC_HOST);
  });

  it("keeps a stored host", () => {
    expect(giteaHostPrefill(GITEA_HOST_KEY, "https://gitea.example")).toBe("https://gitea.example");
  });

  it("leaves other keys untouched, blank or stored", () => {
    expect(giteaHostPrefill("GITEA_CLIENT_ID", "")).toBe("");
    expect(giteaHostPrefill("GITEA_CLIENT_ID", "id-1")).toBe("id-1");
  });
});

describe("giteaApplicationsHref", () => {
  it("falls back to the public service when the host is missing or blank", () => {
    expect(giteaApplicationsHref({})).toBe(GITEA_PUBLIC_APPLICATIONS_URL);
    expect(giteaApplicationsHref({ [GITEA_HOST_KEY]: "" })).toBe(GITEA_PUBLIC_APPLICATIONS_URL);
    expect(giteaApplicationsHref({ [GITEA_HOST_KEY]: "   " })).toBe(GITEA_PUBLIC_APPLICATIONS_URL);
  });

  it("builds the link from the typed host", () => {
    expect(giteaApplicationsHref({ [GITEA_HOST_KEY]: "https://gitea.parity.invalid" })).toBe(
      "https://gitea.parity.invalid/user/settings/applications"
    );
  });

  it("trims a trailing slash off the typed host", () => {
    expect(giteaApplicationsHref({ [GITEA_HOST_KEY]: "https://gitea.example///" })).toBe(
      "https://gitea.example/user/settings/applications"
    );
  });
});

describe("PROVIDER_CREDENTIALS fields", () => {
  it("declares the GitHub keys, types, and required flags", () => {
    const fields = PROVIDER_CREDENTIALS.github.fields;
    expect(fields.map((field) => [field.key, field.type, field.required])).toEqual([
      ["GITHUB_CLIENT_ID", "text", true],
      ["GITHUB_CLIENT_SECRET", "password", true],
      ["GITHUB_ORGANIZATION_ID", "text", false],
    ]);
    expect(fields.every((field) => field.label !== "")).toBe(true);
    expect(fields.every((field) => field.placeholder === undefined)).toBe(true);
  });

  it("declares the GitLab keys with the pinned host placeholder", () => {
    const fields = PROVIDER_CREDENTIALS.gitlab.fields;
    expect(fields.map((field) => [field.key, field.type, field.required])).toEqual([
      ["GITLAB_HOST", "text", true],
      ["GITLAB_CLIENT_ID", "text", true],
      ["GITLAB_CLIENT_SECRET", "password", true],
    ]);
    expect(fields[0]?.placeholder).toBe(GITLAB_PUBLIC_HOST);
    expect(fields[0]?.label).toBe("Host");
    expect(fields[1]?.label).toBe("Application ID");
    expect(fields[2]?.label).toBe("Secret");
  });

  it("declares the Gitea keys with the public-host placeholder", () => {
    const fields = PROVIDER_CREDENTIALS.gitea.fields;
    expect(fields.map((field) => [field.key, field.type, field.required])).toEqual([
      [GITEA_HOST_KEY, "text", true],
      ["GITEA_CLIENT_ID", "text", true],
      ["GITEA_CLIENT_SECRET", "password", true],
    ]);
    expect(fields[0]?.placeholder).toBe(GITEA_PUBLIC_HOST);
    expect(PROVIDER_CREDENTIALS.gitea.prefill).toBe(giteaHostPrefill);
  });

  it("declares the Google keys without a prefill", () => {
    const fields = PROVIDER_CREDENTIALS.google.fields;
    expect(fields.map((field) => [field.key, field.type, field.required])).toEqual([
      ["GOOGLE_CLIENT_ID", "text", true],
      ["GOOGLE_CLIENT_SECRET", "password", true],
    ]);
    expect(PROVIDER_CREDENTIALS.google.prefill).toBeUndefined();
    expect(PROVIDER_CREDENTIALS.github.prefill).toBeUndefined();
    expect(PROVIDER_CREDENTIALS.gitlab.prefill).toBeUndefined();
  });

  it("gives every field help copy, with console links opening a new tab safely", () => {
    for (const config of Object.values(PROVIDER_CREDENTIALS)) {
      for (const field of config.fields) {
        expect(field.description).toBeDefined();
        const { unmount } = render(<>{field.description}</>);
        try {
          const links = screen.queryAllByRole("link");
          for (const link of links) {
            expect(link).toHaveAttribute("target", "_blank");
            expect(link.getAttribute("rel")).toContain("noreferrer");
          }
        } finally {
          unmount();
        }
      }
    }
  });

  it("points field help at the provider consoles and guides", () => {
    const helpHrefs = (slug: keyof typeof PROVIDER_CREDENTIALS): string[] =>
      PROVIDER_CREDENTIALS[slug].fields.flatMap((field) => {
        const { unmount } = render(<>{field.description}</>);
        try {
          return screen.queryAllByRole("link").map((link) => link.getAttribute("href") ?? "");
        } finally {
          unmount();
        }
      });
    expect(helpHrefs("github")).toEqual([GITHUB_OAUTH_CONSOLE_URL, GITHUB_OAUTH_CONSOLE_URL]);
    expect(helpHrefs("gitlab")).toEqual([[], GITLAB_OAUTH_DOCS_URL, GITLAB_OAUTH_DOCS_URL].flat());
    expect(helpHrefs("gitea")).toEqual([[], GITEA_PUBLIC_APPLICATIONS_URL, GITEA_PUBLIC_APPLICATIONS_URL].flat());
    expect(helpHrefs("google")).toEqual([GOOGLE_CLIENT_GUIDE_URL, GOOGLE_SECRET_GUIDE_URL]);
  });
});

describe("PROVIDER_CREDENTIALS copy guidance", () => {
  it("covers the copy labels each provider shell renders", () => {
    expect(Object.keys(PROVIDER_CREDENTIALS.github.copyDescriptions ?? {}).sort()).toEqual([
      "Callback URI",
      "Origin URL",
    ]);
    expect(Object.keys(PROVIDER_CREDENTIALS.gitlab.copyDescriptions ?? {})).toEqual(["Callback URL"]);
    expect(Object.keys(PROVIDER_CREDENTIALS.gitea.copyDescriptions ?? {})).toEqual(["Callback URI"]);
    expect(Object.keys(PROVIDER_CREDENTIALS.google.copyDescriptions ?? {}).sort()).toEqual([
      "Callback URI",
      "Origin URL",
    ]);
  });

  it("renders paste guidance under every copy field", () => {
    for (const config of Object.values(PROVIDER_CREDENTIALS)) {
      for (const description of Object.values(config.copyDescriptions ?? {})) {
        const { unmount } = render(<>{description}</>);
        try {
          expect(screen.queryByRole("link")).not.toBeInTheDocument();
        } finally {
          unmount();
        }
        expect(String(description)).toContain("Paste this address into");
      }
    }
  });

  it("links copy help at the provider consoles", () => {
    expect(PROVIDER_CREDENTIALS.github.copyHelp?.["Origin URL"]).toEqual({
      href: GITHUB_OAUTH_CONSOLE_URL,
      label: "Open the GitHub application console",
    });
    expect(PROVIDER_CREDENTIALS.github.copyHelp?.["Callback URI"]).toEqual({
      href: GITHUB_OAUTH_CONSOLE_URL,
      label: "Open the GitHub application console",
    });
    expect(PROVIDER_CREDENTIALS.gitlab.copyHelp?.["Callback URL"]).toEqual({
      href: GITLAB_OAUTH_DOCS_URL,
      label: "Read the GitLab OAuth guide",
    });
    expect(PROVIDER_CREDENTIALS.google.copyHelp?.["Origin URL"]).toEqual({
      href: GOOGLE_OAUTH_CONSOLE_URL,
      label: "Open the Google credentials console",
    });
    expect(PROVIDER_CREDENTIALS.google.copyHelp?.["Callback URI"]).toEqual({
      href: GOOGLE_OAUTH_CONSOLE_URL,
      label: "Open the Google credentials console",
    });
  });

  it("resolves the Gitea copy help live from the typed host", () => {
    const help = PROVIDER_CREDENTIALS.gitea.copyHelp?.["Callback URI"];
    expect(help?.label).toBe("Open this host's application settings");
    expect(typeof help?.href).toBe("function");
    expect(resolveCopyHelpHref(help?.href, { [GITEA_HOST_KEY]: "https://gitea.parity.invalid" })).toBe(
      "https://gitea.parity.invalid/user/settings/applications"
    );
    expect(resolveCopyHelpHref(help?.href, { [GITEA_HOST_KEY]: "" })).toBe(GITEA_PUBLIC_APPLICATIONS_URL);
  });

  it("repoints the rendered Gitea help link on every draft, never stale", () => {
    const help = PROVIDER_CREDENTIALS.gitea.copyHelp?.["Callback URI"];
    if (help === undefined) throw new Error("gitea copy help is missing");
    const { rerender } = render(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://console.test/auth/gitea/callback/"
        helpHref={help.href}
        helpLabel={help.label}
        fieldValues={{ [GITEA_HOST_KEY]: "https://gitea.parity.invalid" }}
      />
    );
    expect(screen.getByRole("link", { name: help.label })).toHaveAttribute(
      "href",
      "https://gitea.parity.invalid/user/settings/applications"
    );
    rerender(
      <AdminAuthCopyField
        label="Callback URI"
        url="https://console.test/auth/gitea/callback/"
        helpHref={help.href}
        helpLabel={help.label}
        fieldValues={{ [GITEA_HOST_KEY]: "" }}
      />
    );
    expect(screen.getByRole("link", { name: help.label })).toHaveAttribute("href", GITEA_PUBLIC_APPLICATIONS_URL);
  });
});

describe("Gitea form over PROVIDER_CREDENTIALS (ADM-030, NEWFRONT-492)", () => {
  const GITEA_CONFIG = {
    GITEA_HOST: "",
    GITEA_CLIENT_ID: "",
    GITEA_CLIENT_SECRET: "",
    ENABLE_GITEA_SYNC: "0",
  };
  const HELP_LABEL = PROVIDER_CREDENTIALS.gitea.copyHelp?.["Callback URI"]?.label ?? "";

  function renderGiteaForm(): void {
    const gitea = PROVIDER_CREDENTIALS.gitea;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AdminAuthProviderForm
          provider={AUTH_PROVIDER_BY_SLUG.gitea}
          config={GITEA_CONFIG}
          fields={gitea.fields}
          {...(gitea.prefill === undefined ? {} : { prefill: gitea.prefill })}
          {...(gitea.copyHelp === undefined ? {} : { copyHelp: gitea.copyHelp })}
        />
      </QueryClientProvider>
    );
  }

  function helpHref(): string | null {
    return screen.getByRole("link", { name: HELP_LABEL }).getAttribute("href");
  }

  beforeEach(() => {
    const unreachable = (async (url: string) => ({
      status: 404,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve("{}"),
    })) as never;
    createAppClient("https://app.example.test", unreachable);
  });

  afterEach(() => {
    resetAppClient();
  });

  it("follows the typed host live and falls back to the public host the moment it clears", () => {
    renderGiteaForm();
    expect(screen.getByTestId("admin-auth-field-GITEA_HOST")).toHaveValue(GITEA_PUBLIC_HOST);
    expect(helpHref()).toBe(GITEA_PUBLIC_APPLICATIONS_URL);

    fireEvent.change(screen.getByTestId("admin-auth-field-GITEA_HOST"), {
      target: { value: "https://gitea.parity.invalid" },
    });
    expect(helpHref()).toBe("https://gitea.parity.invalid/user/settings/applications");

    fireEvent.change(screen.getByTestId("admin-auth-field-GITEA_HOST"), { target: { value: "" } });
    expect(helpHref()).toBe(GITEA_PUBLIC_APPLICATIONS_URL);
  });
});
