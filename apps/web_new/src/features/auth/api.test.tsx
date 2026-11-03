// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { useSessionStore } from "../../core/session/store.js";
import {
  FINISH_FAILURE_TITLE,
  SINGLE_ANSWER_FAILED_TITLE,
  TOUR_COMPLETE_FAILED_TITLE,
  useAcceptWorkspaceInvitations,
  useAnswerWorkspaceInvitation,
  useApproveDeviceCode,
  useCheckWorkspaceSlug,
  useCompleteTour,
  useCreateJoinRequest,
  useCreateWorkspace,
  useFinishOnboarding,
  useForgotPassword,
  useJoinProject,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordReset,
  usePasswordSignIn,
  usePasswordSignUp,
  useSendWorkspaceInvites,
  useSetPassword,
  useUpdateMe,
  useUploadUserAvatar,
} from "./api.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../shared/shell/index.js";
import { JOIN_PROJECT_FAILED_TITLE } from "./api.js";
import { authKeys } from "./queries.js";
import { SIGN_OUT_FAILED_MESSAGE, SIGN_OUT_FAILED_TITLE, useSignOut } from "./signOut.js";

interface SeenRequest {
  url: string;
  csrf: string | null;
}

const CSRF_URL = "https://api.test/auth/get-csrf-token/";

function recordingTransport(seen: SeenRequest[]): Transport {
  return (url, init) => {
    seen.push({ url, csrf: init.headers["X-CSRFTOKEN"] ?? null });
    if (url === CSRF_URL) {
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "fresh-token" })),
      });
    }
    let body = "";
    if (url.endsWith("/api/v1/auth/device/approve/")) {
      body = JSON.stringify({ ok: true, user_email: "ada@example.com", workspace_slug: "acme" });
    } else if (url.endsWith("/projects/invitations/")) {
      body = JSON.stringify({ message: "joined" });
    } else if (url.endsWith("/auth/forgot-password/")) {
      body = JSON.stringify({ message: "Check your email to reset your password" });
    } else if (url.endsWith("/auth/set-password/")) {
      body = JSON.stringify({
        id: "11111111-1111-4111-8111-111111111111",
        email: "ada@example.com",
        is_password_autoset: false,
      });
    }
    return Promise.resolve({
      status: 200,
      url: "https://api.test/landing",
      headers: { get: () => null },
      text: () => Promise.resolve(body),
    });
  };
}

function wrapper(client: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Provider({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

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

interface Settlement {
  landing?: string | null | undefined;
  error?: unknown;
}

function SignOutProbe({ onSettled }: { onSettled: (outcome: Settlement) => void }): React.ReactElement {
  const signOut = useSignOut();
  return (
    <button
      type="button"
      onClick={() => {
        signOut.mutate(undefined, {
          onSuccess: (landing) => onSettled({ landing }),
          onError: (error) => onSettled({ error }),
        });
      }}
    >
      Sign out
    </button>
  );
}

function renderProbe(transport: Transport, onSettled: (outcome: Settlement) => void): QueryClient {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(["session", "me"], { id: "user-1" });
  render(
    <QueryClientProvider client={queryClient}>
      <SignOutProbe onSettled={onSettled} />
    </QueryClientProvider>
  );
  return queryClient;
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
});

/**
 * Every credential submission drops the cached forgery token first, so the
 * POST carries a freshly fetched one even when the cache is warm from an
 * earlier submit (AUTH-020). Each entry submits twice and counts the
 * token fetches.
 */
describe.each([
  {
    name: "usePasswordSignIn",
    useHook: usePasswordSignIn,
    args: { email: "ada@example.com", password: "secret-1" },
    postPath: "/auth/sign-in/",
  },
  {
    name: "useMagicSignIn",
    useHook: useMagicSignIn,
    args: { email: "ada@example.com", code: "123456" },
    postPath: "/auth/magic-sign-in/",
  },
  {
    name: "usePasswordSignUp",
    useHook: usePasswordSignUp,
    args: { email: "ada@example.com", password: "secret-1", confirmPassword: "secret-1" },
    postPath: "/auth/sign-up/",
  },
  {
    name: "useMagicSignUp",
    useHook: useMagicSignUp,
    args: { email: "ada@example.com", code: "123456" },
    postPath: "/auth/magic-sign-up/",
  },
  {
    name: "useForgotPassword",
    useHook: useForgotPassword,
    args: "ada@example.com",
    postPath: "/auth/forgot-password/",
  },
  {
    name: "usePasswordReset",
    useHook: usePasswordReset,
    args: { uid: "uid", token: "token", password: "Tq7!mZ2@kP9#xR", confirmPassword: "Tq7!mZ2@kP9#xR" },
    postPath: "/auth/reset-password/uid/token/",
  },
  {
    name: "useSetPassword",
    useHook: useSetPassword,
    args: "Tq7!mZ2@kP9#xR",
    postPath: "/auth/set-password/",
  },
  {
    name: "useApproveDeviceCode",
    useHook: useApproveDeviceCode,
    args: "ABCD-1234",
    postPath: "/api/v1/auth/device/approve/",
  },
  {
    name: "useJoinProject",
    useHook: useJoinProject,
    args: { slug: "acme", projectId: "project-1" },
    postPath: "/api/users/me/workspaces/acme/projects/invitations/",
  },
])(
  "$name",
  ({
    useHook,
    args,
    postPath,
  }: {
    useHook: () => { mutateAsync: (input: never) => Promise<unknown> };
    args: unknown;
    postPath: string;
  }) => {
    it("fetches a fresh forgery token for every submission", async () => {
      const seen: SeenRequest[] = [];
      createAppClient("https://api.test", recordingTransport(seen));
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const { result } = renderHook(useHook, { wrapper: wrapper(queryClient) });

      await act(async () => {
        await result.current.mutateAsync(args as never);
      });
      await act(async () => {
        await result.current.mutateAsync(args as never);
      });

      const tokenFetches = seen.filter((request) => request.url === CSRF_URL);
      expect(tokenFetches).toHaveLength(2);
      const posts = seen.filter((request) => request.url.endsWith(postPath));
      expect(posts).toHaveLength(2);
      for (const post of posts) {
        expect(post.csrf).toBe("fresh-token");
      }
    });
  }
);

describe("useJoinProject", () => {
  it("refreshes the workspace subtree after joining", async () => {
    const seen: SeenRequest[] = [];
    createAppClient("https://api.test", recordingTransport(seen));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["ws", "acme", "auth", "project-roles"], {});
    const { result } = renderHook(() => useJoinProject(), { wrapper: wrapper(queryClient) });

    let joined: unknown;
    await act(async () => {
      joined = await result.current.mutateAsync({ slug: "acme", projectId: "project-1" });
    });

    expect(joined).toEqual({ message: "joined" });
    expect(queryClient.getQueryState(["ws", "acme", "auth", "project-roles"])?.isInvalidated).toBe(true);
  });

  it("toasts and rejects when the join fails", async () => {
    const transport = stubTransport((url) => {
      if (url === CSRF_URL) return { status: 200, url, body: CSRF };
      return { status: 500, url, body: "{}" };
    });
    createAppClient("https://api.test", transport);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useJoinProject(), { wrapper: wrapper(queryClient) });

    let rejected: unknown = null;
    await act(async () => {
      try {
        await result.current.mutateAsync({ slug: "acme", projectId: "project-1" });
      } catch (error) {
        rejected = error;
      }
    });

    expect(rejected).toBeInstanceOf(Error);
    expect(notifyError).toHaveBeenCalledWith(JOIN_PROJECT_FAILED_TITLE, expect.any(String));
  });
});

describe("useSignOut", () => {
  it("signs out to the entry route", async () => {
    const settled: Settlement[] = [];
    const queryClient = renderProbe(
      stubTransport(async (url) => ({
        status: 200,
        body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}",
        url,
      })),
      (outcome) => settled.push(outcome)
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out" }));

    await waitFor(() => expect(settled).toHaveLength(1));
    expect(settled[0]).toEqual({ landing: "/" });
    expect(queryClient.getQueryData(["session", "me"])).toBeUndefined();
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and rejects a failed POST without clearing", async () => {
    const settled: Settlement[] = [];
    const queryClient = renderProbe(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/sign-out/")) return { status: 500, body: "{}", url };
        return { status: 200, body: CSRF, url };
      }),
      (outcome) => settled.push(outcome)
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out" }));

    await waitFor(() => expect(settled).toHaveLength(1));
    expect(settled[0]?.error).toBeInstanceOf(Error);
    expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
  });
});

const PROFILE_FIXTURE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "223e4567-e89b-12d3-a456-426614174001",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: false,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "full",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
};

describe("useUpdateMe", () => {
  it("patches the user row and refreshes the cached user", async () => {
    const seen: string[] = [];
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return {
          status: 200,
          body: JSON.stringify({
            id: "123e4567-e89b-12d3-a456-426614174000",
            email: "ada@example.com",
            username: "ada",
            first_name: "Ada",
            last_name: "Byron",
            display_name: "Ada Byron",
            avatar: "",
            user_timezone: "UTC",
          }),
          url,
        };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    const { result } = renderHook(() => useUpdateMe(), { wrapper: wrapper(queryClient) });

    let updated: unknown;
    await act(async () => {
      updated = await result.current.mutateAsync({ first_name: "Ada", last_name: "Byron" });
    });
    expect((updated as { display_name: string }).display_name).toBe("Ada Byron");
    expect(seen).toContain("PATCH https://api.test/api/users/me/");
    expect(queryClient.getQueryState(["session", "me"])?.isInvalidated).toBe(true);
  });
});

describe("useUploadUserAvatar", () => {
  it("uploads through the three legs and resolves the asset URL", async () => {
    createAppClient(
      "https://api.test",
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/assets/v2/user-assets/")) {
          return {
            status: 201,
            body: JSON.stringify({
              asset_id: "asset-1",
              asset_url: "https://cdn.test/avatars/asset-1.png",
              upload_data: { url: "https://uploads.test/signed", fields: { key: "k" } },
            }),
            url,
          };
        }
        return { status: 200, body: "{}", url };
      })
    );
    const realFetch = globalThis.fetch;
    const puts: string[] = [];
    globalThis.fetch = (async (url: string) => {
      puts.push(url);
      return { ok: true, status: 204 };
    }) as unknown as typeof fetch;
    try {
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const { result } = renderHook(() => useUploadUserAvatar(), { wrapper: wrapper(queryClient) });
      let url: unknown;
      await act(async () => {
        url = await result.current.mutateAsync(new File(["bytes"], "ada.png", { type: "image/png" }));
      });
      expect(url).toBe("https://cdn.test/avatars/asset-1.png");
      expect(puts).toEqual(["https://uploads.test/signed"]);
    } finally {
      globalThis.fetch = realFetch;
    }
  });
});

describe("useFinishOnboarding", () => {
  const settings = {
    id: "123e4567-e89b-12d3-a456-426614174000",
    email: "ada@example.com",
    workspace: {
      last_workspace_id: "w1",
      last_workspace_slug: "acme",
      fallback_workspace_id: null,
      fallback_workspace_slug: null,
      invites: 0,
    },
  };

  function finishTransport(seen: string[]) {
    return stubTransport(async (url, init) => {
      seen.push(`${init.method} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
        return { status: 200, body: JSON.stringify(PROFILE_FIXTURE), url };
      }
      if (url.endsWith("/api/users/me/onboard/")) {
        return { status: 200, body: JSON.stringify({ message: "Updated successfully" }), url };
      }
      if (url.endsWith("/api/users/me/settings/")) {
        return { status: 200, body: JSON.stringify(settings), url };
      }
      if (url.endsWith("/api/users/me/profile/")) {
        return { status: 200, body: JSON.stringify(PROFILE_FIXTURE), url };
      }
      return { status: 200, body: "{}", url };
    });
  }

  it("stamps, marks, refreshes, and lands in the last workspace", async () => {
    const seen: string[] = [];
    createAppClient("https://api.test", finishTransport(seen));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(
      ["session", "workspaces"],
      [{ id: "w1", name: "Acme", slug: "acme", logo_url: "", total_members: 1, role: 20 }]
    );
    const { result } = renderHook(() => useFinishOnboarding(), { wrapper: wrapper(queryClient) });

    let landing: unknown;
    await act(async () => {
      landing = await result.current.mutateAsync();
    });
    expect(landing).toBe("/acme");
    expect(seen).toContain("PATCH https://api.test/api/users/me/profile/");
    expect(seen).toContain("PATCH https://api.test/api/users/me/onboard/");
    expect(queryClient.getQueryData(["session", "profile"])).toMatchObject({ is_onboarded: true });
    expect(queryClient.getQueryData(["session", "settings"])).toEqual(settings);
  });

  it("toasts and rejects when a leg fails", async () => {
    createAppClient(
      "https://api.test",
      stubTransport(async (url) => {
        if (url.endsWith("/api/users/me/profile/")) return { status: 500, body: "{}", url };
        return { status: 200, body: CSRF, url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const { result } = renderHook(() => useFinishOnboarding(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await expect(result.current.mutateAsync()).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(FINISH_FAILURE_TITLE, expect.any(String));
  });
});

const CREATED_WORKSPACE_FIXTURE = {
  id: "333e4567-e89b-12d3-a456-426614174002",
  name: "Acme",
  slug: "acme",
  logo: null,
  logo_asset: null,
  logo_url: null,
  owner: "123e4567-e89b-12d3-a456-426614174000",
  organization_size: "2-10",
  timezone: "UTC",
  background_color: "",
  total_members: 1,
  role: 20,
  created_by: null,
  updated_by: null,
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-01-02T03:04:05Z",
};

const WORKSPACE_LIST_FIXTURE = [
  {
    id: "333e4567-e89b-12d3-a456-426614174002",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 1,
    role: 20,
  },
];

const SETTINGS_FIXTURE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  workspace: {
    last_workspace_id: "333e4567-e89b-12d3-a456-426614174002",
    last_workspace_slug: "acme",
    fallback_workspace_id: null,
    fallback_workspace_slug: null,
    invites: 0,
  },
};

describe("useCheckWorkspaceSlug", () => {
  it.each([
    { status: true, expected: true },
    { status: false, expected: false },
  ])("resolves $expected when the server reports status $status", async ({ status, expected }) => {
    const seen: string[] = [];
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        return { status: 200, body: JSON.stringify({ status }), url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useCheckWorkspaceSlug(), { wrapper: wrapper(queryClient) });

    let free: unknown;
    await act(async () => {
      free = await result.current.mutateAsync("acme");
    });
    expect(free).toBe(expected);
    expect(seen).toContain("GET https://api.test/api/workspace-slug-check/?slug=acme");
  });
});

describe("useCreateWorkspace", () => {
  it("creates, stamps the last workspace, and refreshes the list and settings", async () => {
    const seen: string[] = [];
    let profilePatch: unknown;
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/workspaces/") && init.method === "POST") {
          return { status: 201, body: JSON.stringify(CREATED_WORKSPACE_FIXTURE), url };
        }
        if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
          profilePatch = JSON.parse(init.body as string);
          return { status: 200, body: JSON.stringify(PROFILE_FIXTURE), url };
        }
        if (url.endsWith("/api/users/me/workspaces/")) {
          return { status: 200, body: JSON.stringify(WORKSPACE_LIST_FIXTURE), url };
        }
        if (url.endsWith("/api/users/me/settings/")) {
          return { status: 200, body: JSON.stringify(SETTINGS_FIXTURE), url };
        }
        return { status: 200, body: "{}", url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useCreateWorkspace(), { wrapper: wrapper(queryClient) });

    let created: unknown;
    await act(async () => {
      created = await result.current.mutateAsync({ name: "Acme", slug: "acme", organization_size: "2-10" });
    });
    expect((created as { slug: string }).slug).toBe("acme");
    expect(seen).toContain("POST https://api.test/api/workspaces/");
    expect(profilePatch).toEqual({ last_workspace_id: "333e4567-e89b-12d3-a456-426614174002" });
    expect(queryClient.getQueryData(["session", "workspaces"])).toEqual(WORKSPACE_LIST_FIXTURE);
    expect(queryClient.getQueryData(["session", "settings"])).toEqual(SETTINGS_FIXTURE);
  });
});

describe("useAcceptWorkspaceInvitations", () => {
  it("batch-accepts and refreshes the list and settings", async () => {
    const seen: string[] = [];
    let accepted: unknown;
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "POST") {
          accepted = JSON.parse(init.body as string);
          return { status: 204, body: "", url };
        }
        if (url.endsWith("/api/users/me/workspaces/")) {
          return { status: 200, body: JSON.stringify(WORKSPACE_LIST_FIXTURE), url };
        }
        if (url.endsWith("/api/users/me/settings/")) {
          return { status: 200, body: JSON.stringify(SETTINGS_FIXTURE), url };
        }
        return { status: 200, body: "{}", url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAcceptWorkspaceInvitations(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await result.current.mutateAsync(["invite-1", "invite-2"]);
    });
    expect(accepted).toEqual({ invitations: ["invite-1", "invite-2"] });
    expect(queryClient.getQueryData(["session", "workspaces"])).toEqual(WORKSPACE_LIST_FIXTURE);
    expect(queryClient.getQueryData(["session", "settings"])).toEqual(SETTINGS_FIXTURE);
  });
});

describe("useCreateJoinRequest", () => {
  function joinRequestTransport(seen: string[], answer: unknown) {
    return stubTransport(async (url, init) => {
      seen.push(`${init.method} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/workspaces/join-requests/") && init.method === "POST") {
        return { status: 201, body: JSON.stringify(answer), url };
      }
      if (url.endsWith("/api/users/me/workspaces/")) {
        return { status: 200, body: JSON.stringify(WORKSPACE_LIST_FIXTURE), url };
      }
      if (url.endsWith("/api/users/me/settings/")) {
        return { status: 200, body: JSON.stringify(SETTINGS_FIXTURE), url };
      }
      return { status: 200, body: "{}", url };
    });
  }

  it("refreshes the list and settings when the answer names a workspace slug", async () => {
    const seen: string[] = [];
    createAppClient(
      "https://api.test",
      joinRequestTransport(seen, { message: "Already a member", workspace_slug: "acme" })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.myJoinRequests, []);
    const { result } = renderHook(() => useCreateJoinRequest(), { wrapper: wrapper(queryClient) });

    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.mutateAsync("admin@example.com");
    });
    expect(outcome).toEqual({ message: "Already a member", workspace_slug: "acme" });
    expect(seen).toContain("GET https://api.test/api/users/me/workspaces/");
    expect(seen).toContain("GET https://api.test/api/users/me/settings/");
    expect(queryClient.getQueryData(["session", "workspaces"])).toEqual(WORKSPACE_LIST_FIXTURE);
    expect(queryClient.getQueryState(authKeys.myJoinRequests)?.isInvalidated).toBe(true);
  });

  it("skips the refresh for a plain request and still refreshes the request list", async () => {
    const seen: string[] = [];
    createAppClient("https://api.test", joinRequestTransport(seen, { message: "Request sent" }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.myJoinRequests, []);
    const { result } = renderHook(() => useCreateJoinRequest(), { wrapper: wrapper(queryClient) });

    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.mutateAsync("admin@example.com");
    });
    expect(outcome).toEqual({ message: "Request sent" });
    expect(seen).not.toContain("GET https://api.test/api/users/me/workspaces/");
    expect(seen).not.toContain("GET https://api.test/api/users/me/settings/");
    expect(queryClient.getQueryState(authKeys.myJoinRequests)?.isInvalidated).toBe(true);
  });
});

describe("useSendWorkspaceInvites", () => {
  it("posts the rows against the workspace and resolves the message", async () => {
    const seen: string[] = [];
    let posted: unknown;
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/workspaces/acme/invitations/")) {
          posted = JSON.parse(init.body as string);
          return { status: 200, body: JSON.stringify({ message: "Invitations sent" }), url };
        }
        return { status: 200, body: "{}", url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useSendWorkspaceInvites(), { wrapper: wrapper(queryClient) });

    let message: unknown;
    await act(async () => {
      message = await result.current.mutateAsync({
        slug: "acme",
        emails: [{ email: "sam@example.com", role: 15 }],
      });
    });
    expect(message).toBe("Invitations sent");
    expect(posted).toEqual({ emails: [{ email: "sam@example.com", role: 15 }] });
    expect(seen).toContain("POST https://api.test/api/workspaces/acme/invitations/");
  });
});

describe("useCompleteTour", () => {
  it("flips the tour flag and stamps the cached profile", async () => {
    const seen: string[] = [];
    let posted: unknown;
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/tour-completed/")) {
          posted = JSON.parse(init.body as string);
          return { status: 200, body: JSON.stringify({ message: "Tour completed" }), url };
        }
        return { status: 200, body: "{}", url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["session", "profile"], { ...PROFILE_FIXTURE, is_tour_completed: false });
    const { result } = renderHook(() => useCompleteTour(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await result.current.mutateAsync();
    });
    expect(posted).toEqual({ is_tour_completed: true });
    expect(seen).toContain("PATCH https://api.test/api/users/me/tour-completed/");
    expect((queryClient.getQueryData(["session", "profile"]) as { is_tour_completed: boolean }).is_tour_completed).toBe(
      true
    );
  });

  it("leaves a missing cached profile missing", async () => {
    createAppClient(
      "https://api.test",
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return { status: 200, body: JSON.stringify({ message: "Tour completed" }), url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useCompleteTour(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await result.current.mutateAsync();
    });
    expect(queryClient.getQueryData(["session", "profile"])).toBeUndefined();
  });

  it("toasts and rejects when the write fails, keeping the cached flag", async () => {
    createAppClient(
      "https://api.test",
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return { status: 500, body: JSON.stringify({ detail: "boom" }), url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["session", "profile"], { ...PROFILE_FIXTURE, is_tour_completed: false });
    const { result } = renderHook(() => useCompleteTour(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await expect(result.current.mutateAsync()).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(TOUR_COMPLETE_FAILED_TITLE, expect.any(String));
    expect((queryClient.getQueryData(["session", "profile"]) as { is_tour_completed: boolean }).is_tour_completed).toBe(
      false
    );
  });
});

interface CapturedPost {
  url: string;
  method: string;
  body: unknown;
}

describe("useAnswerWorkspaceInvitation", () => {
  function setup(transport: Transport): QueryClient {
    createAppClient("https://api.test", transport);
    return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  }

  it("accepts through the join URL", async () => {
    const posts: CapturedPost[] = [];
    const queryClient = setup(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        posts.push({ url, method: init.method, body: init.body === undefined ? null : JSON.parse(init.body) });
        return { status: 200, body: JSON.stringify({ message: "joined" }), url };
      })
    );
    const { result } = renderHook(() => useAnswerWorkspaceInvitation(), { wrapper: wrapper(queryClient) });

    const answer = await result.current.mutateAsync({
      slug: "acme",
      invitationId: "invite-1",
      accepted: true,
      token: "token-1",
    });

    expect(answer).toEqual({ message: "joined" });
    expect(posts).toEqual([
      {
        url: "https://api.test/api/workspaces/acme/invitations/invite-1/join/",
        method: "POST",
        body: { accepted: true, token: "token-1" },
      },
    ]);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("ignores through the join URL", async () => {
    const posts: CapturedPost[] = [];
    const queryClient = setup(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        posts.push({ url, method: init.method, body: init.body === undefined ? null : JSON.parse(init.body) });
        return { status: 200, body: JSON.stringify({ message: "ignored" }), url };
      })
    );
    const { result } = renderHook(() => useAnswerWorkspaceInvitation(), { wrapper: wrapper(queryClient) });

    await result.current.mutateAsync({ slug: "acme", invitationId: "invite-1", accepted: false, token: "token-1" });

    expect(posts).toEqual([
      {
        url: "https://api.test/api/workspaces/acme/invitations/invite-1/join/",
        method: "POST",
        body: { accepted: false, token: "token-1" },
      },
    ]);
  });

  it("toasts the server detail when the answer fails", async () => {
    const queryClient = setup(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return { status: 403, body: JSON.stringify({ error: "bad token" }), url };
      })
    );
    const { result } = renderHook(() => useAnswerWorkspaceInvitation(), { wrapper: wrapper(queryClient) });

    await expect(
      result.current.mutateAsync({ slug: "acme", invitationId: "invite-1", accepted: true, token: "bogus" })
    ).rejects.toThrow("bad token");
    expect(notifyError).toHaveBeenCalledWith(SINGLE_ANSWER_FAILED_TITLE, "bad token");
  });
});
