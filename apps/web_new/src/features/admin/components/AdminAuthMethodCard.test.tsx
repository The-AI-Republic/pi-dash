// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Method card: pinned name/description/icon, toggle-only versus
// configure versus edit-plus-toggle branches, group lock, and the
// theme-swapped GitHub mark.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AUTH_METHODS, type AuthMethodDef } from "../authMethods.js";
import { resetAdminStores } from "../stores.js";
import { useAdminThemeStore } from "../theme.js";
import { AdminAuthMethodCard } from "./AdminAuthMethodCard.js";

function method(key: string): AuthMethodDef {
  const found = AUTH_METHODS.find((entry) => entry.key === key);
  if (!found) throw new Error(`unknown method ${key}`);
  return found;
}

function renderCard(args: {
  methodKey: string;
  enabled?: boolean;
  configured?: boolean;
  saving?: boolean;
  onToggle?: (next: boolean) => void;
}) {
  const seen: boolean[] = [];
  render(
    <AdminAuthMethodCard
      method={method(args.methodKey)}
      enabled={args.enabled ?? false}
      configured={args.configured ?? true}
      saving={args.saving ?? false}
      onToggle={(next) => {
        seen.push(next);
        args.onToggle?.(next);
      }}
    />
  );
  return seen;
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

describe("AdminAuthMethodCard", () => {
  it("renders the pinned name, description, and an icon", () => {
    renderCard({ methodKey: "unique-codes" });
    const card = screen.getByTestId("admin-auth-card-unique-codes");
    expect(card).toHaveTextContent("Unique codes");
    expect(card).toHaveTextContent(
      "Log in or sign up for Pi Dash using codes sent via email. You need to have set up SMTP to use this method."
    );
    expect(card.querySelector("svg")).not.toBeNull();
  });

  it("keeps magic-link and password cards toggle-only", async () => {
    const user = userEvent.setup();
    for (const key of ["unique-codes", "passwords"]) {
      const { unmount } = render(
        <AdminAuthMethodCard
          method={method(key)}
          enabled={false}
          configured={false}
          saving={false}
          onToggle={() => {}}
        />
      );
      const card = screen.getByTestId(`admin-auth-card-${key}`);
      expect(card.querySelector("a")).toBeNull();
      const toggle = screen.getByTestId(`admin-auth-toggle-${key}`);
      expect(toggle).toHaveAttribute("aria-checked", "false");
      await user.click(toggle);
      unmount();
    }
  });

  it("reports toggle flips with the next state", async () => {
    const user = userEvent.setup();
    const seen = renderCard({ methodKey: "passwords", enabled: true });
    await user.click(screen.getByTestId("admin-auth-toggle-passwords"));
    expect(seen).toEqual([false]);
  });

  it("locks the toggle while a save is in flight", () => {
    const onToggle = vi.fn();
    renderCard({ methodKey: "passwords", enabled: true, saving: true, onToggle });
    expect(screen.getByTestId("admin-auth-toggle-passwords")).toHaveAttribute("aria-disabled", "true");
  });

  it("shows a configure link and no toggle while unconfigured", () => {
    renderCard({ methodKey: "github", enabled: false, configured: false });
    const link = screen.getByTestId("admin-auth-link-github");
    expect(link).toHaveAttribute("href", "/god-mode/authentication/github/");
    expect(link).toHaveTextContent("Configure");
    expect(screen.queryByTestId("admin-auth-toggle-github")).not.toBeInTheDocument();
  });

  it("shows an edit link plus a live toggle once configured", async () => {
    const user = userEvent.setup();
    const seen = renderCard({ methodKey: "github", enabled: true, configured: true });
    const link = screen.getByTestId("admin-auth-link-github");
    expect(link).toHaveAttribute("href", "/god-mode/authentication/github/");
    expect(link).toHaveTextContent("Edit");
    const toggle = screen.getByTestId("admin-auth-toggle-github");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.click(toggle);
    expect(seen).toEqual([false]);
  });

  it("renders no link for an OAuth card without a provider page", () => {
    const github = method("github");
    render(
      <AdminAuthMethodCard
        method={{ ...github, providerSlug: undefined }}
        enabled={false}
        configured={false}
        saving={false}
        onToggle={() => {}}
      />
    );
    expect(screen.queryByTestId("admin-auth-link-github")).not.toBeInTheDocument();
  });

  it("swaps the GitHub mark with the resolved console theme", () => {
    const { unmount, rerender } = render(
      <AdminAuthMethodCard
        method={method("github")}
        enabled={false}
        configured={false}
        saving={false}
        onToggle={() => {}}
      />
    );
    useAdminThemeStore.getState().setMode("light");
    rerender(
      <AdminAuthMethodCard
        method={method("github")}
        enabled={false}
        configured={false}
        saving={false}
        onToggle={() => {}}
      />
    );
    expect(screen.getByTestId("admin-auth-card-github").querySelector("img")?.getAttribute("src")).toContain(
      "github-black"
    );
    useAdminThemeStore.getState().setMode("dark");
    rerender(
      <AdminAuthMethodCard
        method={method("github")}
        enabled={false}
        configured={false}
        saving={false}
        onToggle={() => {}}
      />
    );
    expect(screen.getByTestId("admin-auth-card-github").querySelector("img")?.getAttribute("src")).toContain(
      "github-white"
    );
    unmount();
  });

  it("shows logo-free glyphs on the other OAuth cards", () => {
    for (const key of ["google", "gitlab", "gitea"]) {
      const { unmount } = render(
        <AdminAuthMethodCard
          method={method(key)}
          enabled={false}
          configured={false}
          saving={false}
          onToggle={() => {}}
        />
      );
      const card = screen.getByTestId(`admin-auth-card-${key}`);
      expect(card.querySelector("img")).toBeNull();
      expect(card.querySelector("svg")).not.toBeNull();
      unmount();
    }
  });

  it("falls back to a generic glyph for an unknown method id", () => {
    const magic = method("unique-codes");
    render(
      <AdminAuthMethodCard
        method={{ ...magic, key: "unknown" } as unknown as AuthMethodDef}
        enabled={false}
        configured
        saving={false}
        onToggle={() => {}}
      />
    );
    expect(screen.getByTestId("admin-auth-card-unknown").querySelector("svg")).not.toBeNull();
  });
});
