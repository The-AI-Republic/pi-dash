// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema plus prop passing.
vi.mock("../features/auth/index.js", () => ({
  SignInCard: (props: {
    next?: string;
    unsafeNext?: string;
    initialEmail?: string;
    initialErrorCode?: string | null;
    invitationId?: string;
    invitationSlug?: string;
  }) => (
    <div
      data-testid="sign-in-card"
      data-next={props.next ?? ""}
      data-unsafe-next={props.unsafeNext ?? ""}
      data-email={props.initialEmail ?? ""}
      data-error={props.initialErrorCode ?? ""}
      data-invitation={props.invitationId ?? ""}
      data-slug={props.invitationSlug ?? ""}
    />
  ),
  DesktopSignInScreen: (props: { error?: string; errorDescription?: string }) => (
    <div
      data-testid="desktop-sign-in"
      data-error={props.error ?? ""}
      data-error-description={props.errorDescription ?? ""}
    />
  ),
}));

const platformHarness = vi.hoisted(() => ({ kind: "web" }));

vi.mock("../core/platform/index.js", () => ({
  get platform() {
    return platformHarness;
  },
}));

import { isRedirect } from "@tanstack/react-router";

import { EntryScreen, parseEntrySearch, returnPath, Route } from "./index.js";

describe("parseEntrySearch", () => {
  it("keeps every known param", () => {
    expect(
      parseEntrySearch({
        next: "/acme",
        email: "ada@example.com",
        error_code: "5065",
        invitation_id: "invite-1",
        slug: "acme",
      })
    ).toEqual({
      next: "/acme",
      email: "ada@example.com",
      error_code: "5065",
      invitation_id: "invite-1",
      slug: "acme",
    });
  });

  it("drops invalid params instead of throwing", () => {
    expect(parseEntrySearch({})).toEqual({});
    expect(parseEntrySearch({ next: "", email: "", error_code: 42, invitation_id: "", slug: null })).toEqual({});
  });

  it("forwards an unsafe return path for the server to sanitize (AUTH-016)", () => {
    expect(parseEntrySearch({ next: "https://evil.test/phish" })).toEqual({
      unsafeNext: "https://evil.test/phish",
    });
  });

  it("reads the guard-bounce spelling of the return path", () => {
    expect(parseEntrySearch({ next_path: "/acme" })).toEqual({ next: "/acme" });
    expect(parseEntrySearch({ next_path: "https://evil.test/phish" })).toEqual({
      unsafeNext: "https://evil.test/phish",
    });
  });

  it("keeps a partial search", () => {
    expect(parseEntrySearch({ next: "/acme", error_code: "5900" })).toEqual({ next: "/acme", error_code: "5900" });
  });

  it("keeps the desktop failure landing for the desktop entry to read", () => {
    expect(parseEntrySearch({ error: "access_denied", error_description: "The provider refused." })).toEqual({
      error: "access_denied",
      error_description: "The provider refused.",
    });
    expect(parseEntrySearch({ error: "", error_description: 7 })).toEqual({});
  });
});

describe("returnPath", () => {
  it("prefers the canonical param over the alias", () => {
    expect(returnPath({ next: "/alias", next_path: "/canonical" })).toBe("/canonical");
    expect(returnPath({ next: "/alias" })).toBe("/alias");
    expect(returnPath({})).toBeUndefined();
  });
});

describe("entry guard", () => {
  const me = { is_password_autoset: false };

  function stubContext(values: Record<string, unknown>, search: Record<string, unknown> = {}) {
    return {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            const value = values[options.queryKey[1] as string];
            if (value instanceof Error) throw value;
            if (value === undefined) throw new Error("no fixture");
            return value;
          },
        },
      },
      search,
      location: { href: "/" },
      params: {},
    } as never;
  }

  async function landingOf(args: never): Promise<string> {
    try {
      await Route.options.beforeLoad?.(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      return (error as unknown as { options: { href: string } }).options.href;
    }
    expect.unreachable("expected a redirect");
  }

  it("renders the card for signed-out visitors", async () => {
    const failed = new ApiError({ status: 401, code: "http", message: "signed out" });
    await expect(Route.options.beforeLoad?.(stubContext({ me: failed }))).resolves.toEqual({});
  });

  it("honors the return path for signed-in members", async () => {
    const settings = { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } };
    await expect(
      landingOf(
        stubContext(
          { me, profile: { is_onboarded: true }, settings, workspaces: [{ slug: "acme" }] },
          { next: "/acme/issues" }
        )
      )
    ).resolves.toBe("/acme/issues");
  });

  it("lands signed-in members on their last workspace", async () => {
    const settings = { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } };
    await expect(
      landingOf(stubContext({ me, profile: { is_onboarded: true }, settings, workspaces: [{ slug: "acme" }] }))
    ).resolves.toBe("/acme");
  });

  it("offers workspace creation to members with nowhere to go", async () => {
    const settings = { workspace: { last_workspace_slug: null, fallback_workspace_slug: null } };
    await expect(
      landingOf(stubContext({ me, profile: { is_onboarded: true }, settings, workspaces: [] }))
    ).resolves.toBe("/create-workspace");
  });

  it("funnels unfinished members to onboarding", async () => {
    await expect(landingOf(stubContext({ me, profile: { is_onboarded: false } }))).resolves.toBe("/onboarding");
  });

  it("lands members whose flags are all set even without the marker", async () => {
    const settings = { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } };
    const profile = {
      is_onboarded: false,
      onboarding_step: {
        profile_complete: true,
        workspace_create: true,
        workspace_invite: true,
        workspace_join: true,
      },
    };
    await expect(landingOf(stubContext({ me, profile, settings, workspaces: [{ slug: "acme" }] }))).resolves.toBe(
      "/acme"
    );
  });
});

describe("EntryScreen", () => {
  it("renders the sign-in card with the return path for signed-out visitors", () => {
    render(<EntryScreen next="/acme/" />);
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-next", "/acme/");
  });

  it("passes the card params through to the sign-in card", () => {
    render(
      <EntryScreen
        next="/acme"
        email="ada@example.com"
        errorCode="5065"
        invitationId="invite-1"
        invitationSlug="acme"
      />
    );
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "/acme");
    expect(card).toHaveAttribute("data-unsafe-next", "");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-error", "5065");
    expect(card).toHaveAttribute("data-invitation", "invite-1");
    expect(card).toHaveAttribute("data-slug", "acme");
  });

  it("passes an unsafe return path through to the sign-in card", () => {
    render(<EntryScreen unsafeNext="https://evil.test/phish" />);
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "");
    expect(card).toHaveAttribute("data-unsafe-next", "https://evil.test/phish");
  });
});

describe("entry route component", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    platformHarness.kind = "web";
  });

  function renderEntry(search: {
    next?: string;
    unsafeNext?: string;
    error?: string;
    error_description?: string;
  }): void {
    vi.spyOn(Route, "useSearch").mockReturnValue(search);
    const Component = Route.options.component;
    if (!Component) throw new Error("entry route has no component");
    render(<Component />);
  }

  it("passes a safe return path through to the sign-in card", () => {
    renderEntry({ next: "/acme" });
    expect(screen.getByTestId("sign-in-card")).toHaveAttribute("data-next", "/acme");
  });

  it("passes an unsafe return path through to the sign-in card", () => {
    renderEntry({ unsafeNext: "https://evil.test/phish" });
    const card = screen.getByTestId("sign-in-card");
    expect(card).toHaveAttribute("data-next", "");
    expect(card).toHaveAttribute("data-unsafe-next", "https://evil.test/phish");
  });

  it("renders the desktop sign-in screen on desktop builds", () => {
    platformHarness.kind = "desktop";
    renderEntry({ error: "access_denied", error_description: "The provider refused." });
    const screen_ = screen.getByTestId("desktop-sign-in");
    expect(screen_).toHaveAttribute("data-error", "access_denied");
    expect(screen_).toHaveAttribute("data-error-description", "The provider refused.");
    expect(screen.queryByTestId("sign-in-card")).not.toBeInTheDocument();
  });
});
