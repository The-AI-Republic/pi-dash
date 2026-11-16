// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type ApiClient } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-ins: the desktop screen and the entry card belong to the auth area.
// What this suite owns is the web/desktop fork, the edition seam and the
// guard rule.
vi.mock("../features/auth/components/EntryScreen.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/auth/components/EntryScreen.js")>()),
  EntryScreen: (props: { next?: string; unsafeNext?: string; email?: string; errorCode?: string }) => (
    <div
      data-testid="entry-screen"
      data-next={props.next ?? ""}
      data-unsafe-next={props.unsafeNext ?? ""}
      data-email={props.email ?? ""}
      data-error-code={props.errorCode ?? ""}
    />
  ),
}));

vi.mock("../features/auth/components/DesktopSignInScreen.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/auth/components/DesktopSignInScreen.js")>()),
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

import { edition } from "../core/edition/index.js";
import { meQueryOptions, profileQueryOptions, sessionKeys } from "../core/session/queries.js";
import { parseLoginSearch } from "../features/auth/index.js";
import { redirectRetiredSignIn, rendersLoginInPlace, Route } from "./login.js";

interface ThrownRedirect extends Response {
  options: { to?: unknown; href?: unknown; search?: unknown };
}

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function unauthorizedClient(): ApiClient {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status: 401,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ detail: "signed out" }),
    }),
  });
}

function seedSession(client: QueryClient, apiClient: ApiClient, onboarded: boolean) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
  client.setQueryData(profileQueryOptions(apiClient).queryKey, { is_onboarded: onboarded } as never);
}

async function load(
  client: QueryClient,
  apiClient: ApiClient,
  search: Record<string, unknown>
): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({
    context: { queryClient: client, apiClient },
    location: { href: "/login" },
    search,
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("parseLoginSearch", () => {
  it("reads the return path in both spellings and hands the desktop landing the canonical one", () => {
    expect(parseLoginSearch({ next_path: "/acme/issues" })).toEqual({ next: "/acme/issues", nextPath: "/acme/issues" });
    expect(parseLoginSearch({ next: "/acme" })).toEqual({ next: "/acme", nextPath: "/acme" });
    // The `next` spelling wins over `next_path`, as at the root entry.
    expect(parseLoginSearch({ next_path: "/acme", next: "/other" })).toEqual({
      next: "/other",
      nextPath: "/other",
    });
  });

  it("forwards an unsafe target for the server to sanitize (AUTH-016)", () => {
    expect(parseLoginSearch({ next_path: "https://evil.test/x" })).toEqual({ unsafeNext: "https://evil.test/x" });
    expect(parseLoginSearch({ next: "//evil.test" })).toEqual({ unsafeNext: "//evil.test" });
  });

  it("keeps the entry and failure params, dropping invalid ones", () => {
    expect(
      parseLoginSearch({
        email: "ada@example.com",
        error_code: "5065",
        invitation_id: "invite-1",
        slug: "acme",
        error: "access_denied",
        error_description: "The provider refused.",
      })
    ).toEqual({
      email: "ada@example.com",
      error_code: "5065",
      invitation_id: "invite-1",
      slug: "acme",
      error: "access_denied",
      errorDescription: "The provider refused.",
    });
    expect(parseLoginSearch({})).toEqual({});
    expect(parseLoginSearch({ email: "", error_code: 7, slug: null, error: "" })).toEqual({});
  });
});

describe("login address", () => {
  it("keeps the visitor's search string untouched so next_path survives", () => {
    const validate = Route.options.validateSearch as (search: Record<string, unknown>) => Record<string, unknown>;
    const search = { next_path: "/docs" };
    expect(validate(search)).toBe(search);
    expect(validate({})).toEqual({});
  });
});

describe("redirectRetiredSignIn", () => {
  it("targets the home route", () => {
    let thrown: unknown;
    try {
      redirectRetiredSignIn();
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Response);
    expect((thrown as ThrownRedirect).options.to).toBe("/");
  });
});

describe("login guard", () => {
  afterEach(() => {
    platformHarness.kind = "web";
    delete edition.routes;
    delete edition.slots;
  });

  it("serves /login in place as soon as the edition declares the address (CLOUD-002)", async () => {
    edition.routes = [{ path: "login", scope: "top-level" }];
    expect(rendersLoginInPlace(edition)).toBe(true);
    await expect(load(queryClient(), unauthorizedClient(), { next_path: "/acme" })).resolves.toEqual({ ok: {} });
  });

  it("keeps the retired-address redirect when the edition declares nothing", async () => {
    const outcome = await load(queryClient(), unauthorizedClient(), {});
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ to: "/" });
    expect(rendersLoginInPlace(edition)).toBe(false);
  });

  it("accepts the leading-slash spelling of the declaration too", () => {
    edition.routes = [{ path: "/login", scope: "top-level" }];
    expect(rendersLoginInPlace(edition)).toBe(true);
  });

  it("ignores a workspace route that happens to be named login", () => {
    edition.routes = [{ path: "login" }];
    expect(rendersLoginInPlace(edition)).toBe(false);
  });

  it("never renders in place on the desktop build", () => {
    platformHarness.kind = "desktop";
    edition.routes = [{ path: "login", scope: "top-level" }];
    edition.slots = { cloudLoginCard: () => <p>cloud card</p> };
    expect(rendersLoginInPlace(edition)).toBe(false);
  });

  it("redirects bare to the entry on web builds", async () => {
    const outcome = await load(queryClient(), unauthorizedClient(), {
      error: "access_denied",
      error_description: "The provider refused.",
      next_path: "/acme",
    });
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ to: "/" });
    expect((outcome as { thrown: ThrownRedirect }).thrown.options.search).toBeUndefined();
  });

  it("renders in place when the edition fills the cloud login card (CLOUD-011)", () => {
    const Card = () => <div data-testid="cloud-card" />;
    edition.slots = { cloudLoginCard: Card };
    expect(rendersLoginInPlace(edition)).toBe(true);
  });

  it("keeps the redirect when the edition leaves the card unfilled", () => {
    edition.slots = {};
    expect(rendersLoginInPlace(edition)).toBe(false);
  });

  it("keeps the redirect on desktop even with a filled card", () => {
    platformHarness.kind = "desktop";
    edition.slots = { cloudLoginCard: () => <div /> };
    expect(rendersLoginInPlace(edition)).toBe(false);
  });

  it("serves cloud /login in place and keeps the query string", async () => {
    edition.slots = { cloudLoginCard: () => <div data-testid="cloud-card" /> };
    await expect(load(queryClient(), unauthorizedClient(), { next_path: "/docs" })).resolves.toEqual({ ok: {} });
  });

  it("renders the desktop screen for signed-out visitors on desktop", async () => {
    platformHarness.kind = "desktop";
    await expect(load(queryClient(), unauthorizedClient(), {})).resolves.toEqual({ ok: {} });
  });

  it("forwards signed-in members to their landing on desktop", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedSession(client, apiClient, true);
    const outcome = await load(client, apiClient, { next_path: "/acme" });
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/acme" });
  });

  it("forwards a signed-in cloud member to the carried target", async () => {
    edition.routes = [{ path: "login", scope: "top-level" }];
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedSession(client, apiClient, true);
    const outcome = await load(client, apiClient, { next_path: "/acme/issues" });
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/acme/issues" });
  });

  it("funnels unfinished members to onboarding on desktop", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedSession(client, apiClient, false);
    const outcome = await load(client, apiClient, {});
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/onboarding" });
  });

  it("lands members whose onboarding flags are all set even without the marker", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    client.setQueryData(profileQueryOptions(apiClient).queryKey, {
      is_onboarded: false,
      onboarding_step: {
        profile_complete: true,
        workspace_create: true,
        workspace_invite: true,
        workspace_join: true,
      },
    } as never);
    client.setQueryData(sessionKeys.settings, {
      workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null },
    });
    client.setQueryData(sessionKeys.workspaces, [{ slug: "acme" }]);
    const outcome = await load(client, apiClient, {});
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/acme" });
  });
});

describe("LoginScreen", () => {
  afterEach(() => {
    platformHarness.kind = "web";
    delete edition.routes;
    delete edition.slots;
    vi.restoreAllMocks();
  });

  function renderScreen(search: Record<string, unknown>): HTMLElement {
    vi.spyOn(Route, "useSearch").mockReturnValue(search);
    const Component = Route.options.component;
    if (!Component) throw new Error("login route has no component");
    return render(<Component />).container;
  }

  it("renders the desktop sign-in screen on desktop builds", () => {
    platformHarness.kind = "desktop";
    renderScreen({ error: "access_denied", error_description: "The provider refused." });
    const screen_ = screen.getByTestId("desktop-sign-in");
    expect(screen_).toHaveAttribute("data-error", "access_denied");
    expect(screen_).toHaveAttribute("data-error-description", "The provider refused.");
  });

  it("renders the edition's cloud login card in place (CLOUD-011)", () => {
    edition.slots = { cloudLoginCard: () => <div data-testid="cloud-card">cloud sign-in</div> };
    render(
      (() => {
        vi.spyOn(Route, "useSearch").mockReturnValue({});
        const Component = Route.options.component;
        if (!Component) throw new Error("login route has no component");
        return <Component />;
      })()
    );
    expect(screen.getByTestId("cloud-card")).toBeVisible();
    expect(screen.getByTestId("cloud-card")).toHaveTextContent("cloud sign-in");
  });

  it("renders nothing on web builds", () => {
    const { container } = render(
      (() => {
        vi.spyOn(Route, "useSearch").mockReturnValue({});
        const Component = Route.options.component;
        if (!Component) throw new Error("login route has no component");
        return <Component />;
      })()
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the edition's cloud sign-in card when the slot is filled", () => {
    edition.routes = [{ path: "login", scope: "top-level" }];
    edition.slots = { cloudLoginCard: () => <p>cloud organization sign-in</p> };
    expect(renderScreen({})).toHaveTextContent("cloud organization sign-in");
  });

  it("renders the entry card with the carried target until the cloud card lands", () => {
    edition.routes = [{ path: "login", scope: "top-level" }];
    const container = renderScreen({ next_path: "/acme/issues" });
    const entry = screen.getByTestId("entry-screen");
    expect(container).toContainElement(entry);
    expect(entry).toHaveAttribute("data-next", "/acme/issues");
  });

  it("passes the entry params through to the fallback card", () => {
    edition.routes = [{ path: "login", scope: "top-level" }];
    renderScreen({
      next: "/acme",
      email: "ada@example.com",
      error_code: "5065",
      invitation_id: "invite-1",
      slug: "acme",
    });
    const entry = screen.getByTestId("entry-screen");
    expect(entry).toHaveAttribute("data-next", "/acme");
    expect(entry).toHaveAttribute("data-email", "ada@example.com");
    expect(entry).toHaveAttribute("data-error-code", "5065");
  });

  it("still renders nothing on a community web build", () => {
    expect(renderScreen({})).toBeEmptyDOMElement();
  });
});
