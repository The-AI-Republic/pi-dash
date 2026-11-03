// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit, WorkspaceInvitation } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));

// The not-found state needs fetched detail plus a fetch error at once,
// which a live query never produces, so the suite can pin the invitation
// hook for that one state while every other test uses the real hooks.
const { invitationHookOverride } = vi.hoisted(() => ({
  invitationHookOverride: { current: null as null | (() => unknown) },
}));

vi.mock("../invitations.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../invitations.js")>();
  return {
    ...original,
    useWorkspaceInvitation: (slug?: string, invitationId?: string) =>
      invitationHookOverride.current?.() ?? original.useWorkspaceInvitation(slug, invitationId),
  };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { SINGLE_ANSWER_FAILED_TITLE } from "../api.js";
import { COMMUNITY_URL, GITHUB_STAR_URL, SingleInvitation } from "./SingleInvitation.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

const SINGLE_INVITE_ID = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

function singleInvitation(overrides: Partial<WorkspaceInvitation> = {}): WorkspaceInvitation {
  return {
    id: SINGLE_INVITE_ID,
    workspace: {
      id: "22222222-2222-4222-8222-222222222222",
      name: "Acme",
      slug: "acme",
      logo_url: null,
    },
    email: "ada@example.com",
    accepted: false,
    token: "token-1",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: "https://app.test/workspace-invitations?invitation_id=invite-1",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function meBody(email: string): string {
  return JSON.stringify({
    id: "55555555-5555-4555-8555-555555555555",
    email,
    username: "ada",
    first_name: "Ada",
    last_name: "Lovelace",
    display_name: "Ada Lovelace",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2026-01-01T00:00:00Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "",
    last_login_time: null,
  });
}

interface SingleSetup {
  invitation: WorkspaceInvitation | null;
  invitationStatus?: number;
  viewerEmail?: string | undefined;
  answerStatus?: number;
  onPost?: ((body: unknown) => void) | undefined;
  holdAnswer?: { release: () => void } | undefined;
}

function singleTransport(setup: SingleSetup): Transport {
  let releaseAnswer: (() => void) | null = null;
  const gate = new Promise<void>((resolve) => {
    releaseAnswer = resolve;
  });
  if (setup.holdAnswer && releaseAnswer) {
    setup.holdAnswer.release = releaseAnswer;
  }
  return stubTransport(async (url, init) => {
    if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
    if (url.endsWith("/api/users/me/")) {
      if (setup.viewerEmail === undefined) return { status: 401, body: JSON.stringify({ error: "signed out" }), url };
      return { status: 200, body: meBody(setup.viewerEmail), url };
    }
    if (url.endsWith("/join/") && init.method === "GET") {
      const status = setup.invitationStatus ?? 200;
      if (status !== 200 || setup.invitation === null) {
        return { status, body: JSON.stringify({ error: "gone" }), url };
      }
      return { status: 200, body: JSON.stringify(setup.invitation), url };
    }
    if (url.endsWith("/join/") && init.method === "POST") {
      setup.onPost?.(init.body === undefined ? null : JSON.parse(init.body));
      if (setup.holdAnswer) await gate;
      const status = setup.answerStatus ?? 200;
      if (status !== 200) return { status, body: JSON.stringify({ error: "bad token" }), url };
      return { status: 200, body: JSON.stringify({ message: "answered" }), url };
    }
    return { status: 404, body: JSON.stringify({ error: "unexpected" }), url };
  });
}

function renderSingle(
  transport: Transport,
  props: { invitationId?: string | undefined; slug?: string | undefined; token?: string | undefined } = {
    invitationId: SINGLE_INVITE_ID,
    slug: "acme",
    token: "token-1",
  }
): void {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SingleInvitation invitationId={props.invitationId} slug={props.slug} token={props.token} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
  invitationHookOverride.current = null;
});

describe("SingleInvitation", () => {
  it("loads the emailed invitation", async () => {
    let release!: (invitation: WorkspaceInvitation) => void;
    const gate = new Promise<WorkspaceInvitation>((resolve) => {
      release = resolve;
    });
    renderSingle(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/")) return { status: 200, body: meBody("ada@example.com"), url };
        if (url.endsWith("/join/") && init.method === "GET") {
          return { status: 200, body: JSON.stringify(await gate), url };
        }
        return { status: 404, body: JSON.stringify({ error: "unexpected" }), url };
      })
    );

    expect(screen.getByRole("status")).not.toBeNull();
    expect(screen.getByText("Loading invitation")).not.toBeNull();
    release(singleInvitation());
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
  });

  it("renders the pending card naming the workspace", async () => {
    renderSingle(singleTransport({ invitation: singleInvitation(), viewerEmail: "ada@example.com" }));

    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    expect(screen.getByText("Sent to ada@example.com.")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Accept" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "Ignore" })).not.toBeNull();
  });

  it("accepts a matching address into the joined workspace", async () => {
    const posts: unknown[] = [];
    renderSingle(
      singleTransport({
        invitation: singleInvitation(),
        viewerEmail: "ada@example.com",
        onPost: (body) => posts.push(body),
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept" }));

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/acme" }));
    expect(posts).toEqual([{ accepted: true, token: "token-1" }]);
  });

  it("lands a mismatched address home after accepting", async () => {
    renderSingle(singleTransport({ invitation: singleInvitation(), viewerEmail: "grace@example.com" }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept" }));

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });

  it("accepts without a token by posting an empty one", async () => {
    const posts: unknown[] = [];
    renderSingle(
      singleTransport({
        invitation: singleInvitation(),
        viewerEmail: "ada@example.com",
        onPost: (body) => posts.push(body),
      }),
      {
        invitationId: SINGLE_INVITE_ID,
        slug: "acme",
        token: undefined,
      }
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept" }));

    await waitFor(() => expect(posts).toEqual([{ accepted: true, token: "" }]));
  });

  it("ignores through the join URL and always lands home", async () => {
    const posts: unknown[] = [];
    renderSingle(
      singleTransport({
        invitation: singleInvitation(),
        viewerEmail: "ada@example.com",
        onPost: (body) => posts.push(body),
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Ignore" }));

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
    expect(posts).toEqual([{ accepted: false, token: "token-1" }]);
  });

  it("leaves ignoring inert without the emailed token", async () => {
    const posts: unknown[] = [];
    renderSingle(
      singleTransport({
        invitation: singleInvitation(),
        viewerEmail: "ada@example.com",
        onPost: (body) => posts.push(body),
      }),
      {
        invitationId: SINGLE_INVITE_ID,
        slug: "acme",
        token: undefined,
      }
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Ignore" }));

    await waitFor(() => expect(posts).toEqual([]));
    expect(navigate).not.toHaveBeenCalled();
  });

  it("shows progress on the clicked accept action while answering", async () => {
    const holdAnswer = { release: () => undefined };
    renderSingle(singleTransport({ invitation: singleInvitation(), viewerEmail: "ada@example.com", holdAnswer }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept" }));

    const accept = await screen.findByRole("button", { name: "Accept" });
    await waitFor(() => expect(accept).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByRole("button", { name: "Ignore" })).not.toHaveAttribute("aria-busy", "true");
    holdAnswer.release();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/acme" }));
  });

  it("shows progress on the clicked ignore action while answering", async () => {
    const holdAnswer = { release: () => undefined };
    renderSingle(singleTransport({ invitation: singleInvitation(), viewerEmail: "ada@example.com", holdAnswer }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Ignore" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Ignore" })).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByRole("button", { name: "Accept" })).not.toHaveAttribute("aria-busy", "true");
    holdAnswer.release();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });

  it("stays on a failed answer with an error toast", async () => {
    renderSingle(
      singleTransport({ invitation: singleInvitation(), viewerEmail: "ada@example.com", answerStatus: 403 })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(SINGLE_ANSWER_FAILED_TITLE, "bad token"));
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Acme" })).not.toBeNull();
  });

  it("renders the already-member card for an answered-plus-accepted link", async () => {
    const answered = singleInvitation({ accepted: true, responded_at: "2026-01-02T00:00:00Z" });
    renderSingle(singleTransport({ invitation: answered, viewerEmail: undefined }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText(/already a member/)).not.toBeNull());
    expect(screen.queryByRole("button", { name: "Accept" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ignore" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Continue to home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });

  it("renders the inactive card with a way home for signed-in viewers", async () => {
    const declined = singleInvitation({ accepted: false, responded_at: "2026-01-02T00:00:00Z" });
    renderSingle(singleTransport({ invitation: declined, viewerEmail: "ada@example.com" }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText(/not active anymore/)).not.toBeNull());
    expect(screen.getByRole("link", { name: "Star us on GitHub" })).toHaveAttribute("href", GITHUB_STAR_URL);
    expect(screen.getByRole("link", { name: "Join our community of active creators" })).toHaveAttribute(
      "href",
      COMMUNITY_URL
    );
    await user.click(screen.getByRole("button", { name: "Continue to home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });

  it("renders the inactive card with a sign-in action for visitors", async () => {
    const declined = singleInvitation({ accepted: false, responded_at: "2026-01-02T00:00:00Z" });
    renderSingle(singleTransport({ invitation: declined, viewerEmail: undefined }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText(/not active anymore/)).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Sign in to continue" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });

  it("renders the inactive card when the fetch fails", async () => {
    renderSingle(singleTransport({ invitation: null, invitationStatus: 404, viewerEmail: "ada@example.com" }));

    await waitFor(() => expect(screen.getByText(/not active anymore/)).not.toBeNull());
    expect(screen.queryByRole("button", { name: "Accept" })).toBeNull();
  });

  it("renders the dead not-found branch for detail plus a fetch error at once", async () => {
    invitationHookOverride.current = () => ({ data: singleInvitation(), isError: true, isPending: false });
    renderSingle(singleTransport({ invitation: singleInvitation(), viewerEmail: "ada@example.com" }));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Invitation not found" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Back to home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });
});
