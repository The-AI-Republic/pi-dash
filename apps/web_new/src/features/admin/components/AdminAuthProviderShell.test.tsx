// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider shell: the borderless identity card with the master toggle,
// the provider tab title, the theme-swapped GitHub mark, and the body
// skeleton until configuration loads.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AUTH_PROVIDER_BY_SLUG } from "../providerAuth.js";
import { resetAdminStores } from "../stores.js";
import { useAdminThemeStore } from "../theme.js";
import { AdminAuthProviderShell } from "./AdminAuthProviderShell.js";

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

function renderShell(slug: "github" | "gitlab" | "gitea" | "google", overrides?: { ready?: boolean }) {
  const onToggle = vi.fn();
  render(
    <AdminAuthProviderShell
      provider={AUTH_PROVIDER_BY_SLUG[slug]}
      enabled={false}
      disabled={false}
      onToggle={onToggle}
      ready={overrides?.ready ?? false}
    >
      <div data-testid="provider-form">form</div>
    </AdminAuthProviderShell>
  );
  return { onToggle };
}

describe("provider shell", () => {
  it("renders the provider identity with the tab title", () => {
    renderShell("github", { ready: true });
    expect(screen.getByTestId("admin-auth-provider-name")).toHaveTextContent("GitHub");
    expect(screen.getByTestId("admin-auth-provider-description")).toHaveTextContent(
      "Allow members to login or sign up to pi dash with their GitHub accounts."
    );
    expect(document.title).toBe("GitHub Authentication - God Mode");
    expect(screen.getByTestId("provider-form")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-auth-provider-skeleton")).not.toBeInTheDocument();
  });

  it("shows the skeleton body with no form before configuration loads", () => {
    renderShell("gitlab");
    const skeleton = screen.getByTestId("admin-auth-provider-skeleton");
    expect(skeleton).toHaveAttribute("role", "status");
    expect(skeleton.children).toHaveLength(5);
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-provider-name")).toHaveTextContent("GitLab");
  });

  it("flips the master toggle through the caller", () => {
    const { onToggle } = renderShell("gitea", { ready: true });
    fireEvent.click(screen.getByTestId("admin-auth-provider-toggle"));
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it("locks the master toggle when disabled", () => {
    render(
      <AdminAuthProviderShell provider={AUTH_PROVIDER_BY_SLUG.google} enabled={true} disabled onToggle={() => {}} ready>
        <div data-testid="provider-form">form</div>
      </AdminAuthProviderShell>
    );
    const toggle = screen.getByTestId("admin-auth-provider-toggle");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(toggle).toHaveAttribute("aria-disabled", "true");
  });

  it("swaps the GitHub mark with the resolved console theme", () => {
    useAdminThemeStore.getState().setMode("light");
    const { rerender } = render(
      <AdminAuthProviderShell
        provider={AUTH_PROVIDER_BY_SLUG.github}
        enabled={false}
        disabled={false}
        onToggle={() => {}}
        ready
      >
        {null}
      </AdminAuthProviderShell>
    );
    expect(screen.getByTestId("admin-auth-provider-card").querySelector("img")).toHaveAttribute(
      "src",
      "/logos/github-black.png"
    );
    useAdminThemeStore.getState().setMode("dark");
    rerender(
      <AdminAuthProviderShell
        provider={AUTH_PROVIDER_BY_SLUG.github}
        enabled={false}
        disabled={false}
        onToggle={() => {}}
        ready
      >
        {null}
      </AdminAuthProviderShell>
    );
    expect(screen.getByTestId("admin-auth-provider-card").querySelector("img")).toHaveAttribute(
      "src",
      "/logos/github-white.png"
    );
  });

  it("renders a glyph for each non-GitHub provider", () => {
    for (const slug of ["gitlab", "gitea", "google"] as const) {
      const { unmount } = render(
        <AdminAuthProviderShell
          provider={AUTH_PROVIDER_BY_SLUG[slug]}
          enabled={false}
          disabled={false}
          onToggle={() => {}}
          ready
        >
          {null}
        </AdminAuthProviderShell>
      );
      expect(screen.getByTestId("admin-auth-provider-card").querySelector("svg")).not.toBeNull();
      expect(screen.getByTestId("admin-auth-provider-card").querySelector("img")).toBeNull();
      unmount();
    }
  });
});
