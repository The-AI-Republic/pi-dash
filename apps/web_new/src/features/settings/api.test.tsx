// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings writes (parity/specs/profile.md §6): each hook's request, its
// cache refresh, its feedback, and the failure mapping.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifySuccess, notifyError };
});

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/index.js";
import {
  AVATAR_REMOVE_FAILED_MESSAGE,
  AVATAR_REMOVE_FAILED_TITLE,
  AVATAR_REMOVED_MESSAGE,
  AVATAR_REMOVED_TITLE,
  PROFILE_SAVE_FAILED_MESSAGE,
  PROFILE_SAVED_MESSAGE,
  PROFILE_SAVED_TITLE,
  profileSaveFailureMessage,
  useChangePassword,
  useCheckEmailTaken,
  useConfirmEmailChange,
  useGenerateEmailCode,
  useRemoveAvatar,
  useSaveProfile,
  useStoreAvatar,
  useUploadAvatar,
} from "./api.js";

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function stubTransport(
  handler: (
    url: string,
    init: TransportInit
  ) => { status: number; body?: unknown } | Promise<{ status: number; body?: unknown }>
): { transport: Transport; calls: Call[] } {
  const calls: Call[] = [];
  const transport: Transport = async (url, init) => {
    const parsed = typeof init.body === "string" ? (JSON.parse(init.body) as unknown) : init.body;
    calls.push({ method: init.method ?? "GET", url, body: parsed });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    const result = await handler(url, init);
    return {
      status: result.status,
      url,
      headers: { get: () => null },
      text: async () => (result.body === undefined ? "" : JSON.stringify(result.body)),
    };
  };
  return { transport, calls };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function setup(transport: Transport) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, wrapper: wrapper(queryClient) };
}

const ME_ROW = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "ada",
  avatar: "",
  avatar_url: null,
};

beforeEach(() => {
  notifySuccess.mockClear();
  notifyError.mockClear();
});

afterEach(() => resetAppClient());

describe("profileSaveFailureMessage", () => {
  it("prefers the first field line, then an array field, then the message", () => {
    expect(profileSaveFailureMessage({ fields: { first_name: ["First name is invalid"] } })).toBe(
      "First name is invalid"
    );
    expect(profileSaveFailureMessage({ fields: { first_name: "First name is invalid" } })).toBe(
      "First name is invalid"
    );
    expect(profileSaveFailureMessage({ fields: { first_name: [] }, message: "Server said no" })).toBe("Server said no");
    expect(profileSaveFailureMessage({ fields: null, message: "" })).toBe(PROFILE_SAVE_FAILED_MESSAGE);
    expect(profileSaveFailureMessage(null)).toBe(PROFILE_SAVE_FAILED_MESSAGE);
  });
});

const updatedMe = {
  id: ME_ROW.id,
  email: ME_ROW.email,
  username: "ada",
  first_name: "Ada",
  last_name: "",
  display_name: "ada",
  avatar: "",
  user_timezone: "UTC",
};

describe("useSaveProfile", () => {
  it("patches the names, toasts and merges the answer into the cached row", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: updatedMe }));
    const { queryClient, wrapper: Wrapper } = setup(transport);
    queryClient.setQueryData(sessionKeys.me, { ...ME_ROW });
    const { result } = renderHook(() => useSaveProfile(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ first_name: "Ada", last_name: "", display_name: "ada" });
    });
    expect(calls.find((call) => call.url.endsWith("/api/users/me/") && call.method === "PATCH")?.body).toEqual({
      first_name: "Ada",
      last_name: "",
      display_name: "ada",
    });
    expect(notifySuccess).toHaveBeenCalledWith(PROFILE_SAVED_TITLE, PROFILE_SAVED_MESSAGE);
    await waitFor(() =>
      expect(queryClient.getQueryData(sessionKeys.me)).toMatchObject({ email: "ada@example.com", display_name: "ada" })
    );
  });

  it("leaves an absent cached row absent and toasts the server field line", async () => {
    const { transport } = stubTransport(() => ({
      status: 400,
      body: { first_name: ["This field is required."] },
    }));
    const { queryClient, wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useSaveProfile(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ first_name: "", last_name: "", display_name: "ada" }).catch(() => undefined);
    });
    expect(notifyError).toHaveBeenCalledWith("Could not update your profile", "This field is required.");
    expect(queryClient.getQueryData(sessionKeys.me)).toBeUndefined();
  });
});

describe("useUploadAvatar", () => {
  it("runs the three upload legs and refreshes the member row", async () => {
    const { transport, calls } = stubTransport((url) => {
      if (url.endsWith("/api/assets/v2/user-assets/")) {
        return {
          status: 200,
          body: {
            upload_data: { url: "https://files.test/put", fields: { key: "k" } },
            asset_id: "asset-1",
            asset_url: "/api/assets/v2/static/asset-1/",
          },
        };
      }
      return { status: 204 };
    });
    const { queryClient, wrapper: Wrapper } = setup(transport);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const put = vi.fn(async () => ({ ok: true, status: 204 }));
    vi.stubGlobal("fetch", put);
    try {
      const { result } = renderHook(() => useUploadAvatar(), { wrapper: Wrapper });
      const file = new File(["x"], "photo.png", { type: "image/png" });
      await act(async () => {
        await result.current.mutateAsync(file);
      });
      expect(put).toHaveBeenCalledWith("https://files.test/put", expect.objectContaining({ method: "POST" }));
      expect(calls.some((call) => call.url.endsWith("/api/assets/v2/user-assets/asset-1/"))).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sessionKeys.me });
  });
});

describe("useStoreAvatar", () => {
  it("writes the avatar reference and toasts", async () => {
    const { transport, calls } = stubTransport(() => ({
      status: 200,
      body: { ...updatedMe, avatar: "/api/assets/v2/static/a/" },
    }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useStoreAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync("/api/assets/v2/static/a/");
    });
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ avatar: "/api/assets/v2/static/a/" });
    expect(notifySuccess).toHaveBeenCalledWith(PROFILE_SAVED_TITLE, PROFILE_SAVED_MESSAGE);
  });

  it("toasts a failed store", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { error: "boom" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useStoreAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync("/api/assets/v2/static/a/").catch(() => undefined);
    });
    expect(notifyError).toHaveBeenCalled();
  });
});

describe("useRemoveAvatar", () => {
  it("goes through the v2 asset route for an asset reference", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { ...updatedMe, avatar: "" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useRemoveAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ kind: "asset", assetId: "abc" });
    });
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes("/api/assets/v2/user-assets/abc/"))).toBe(
      true
    );
    expect(calls.filter((call) => call.method === "PATCH").at(-1)?.body).toEqual({ avatar: "" });
    expect(notifySuccess).toHaveBeenCalledWith(AVATAR_REMOVED_TITLE, AVATAR_REMOVED_MESSAGE);
  });

  it("goes through the legacy user file route for a plain key", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { ...updatedMe, avatar: "" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useRemoveAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ kind: "legacy", assetKey: "key.png" });
    });
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes("/api/users/file-assets/key.png/"))).toBe(
      true
    );
  });

  it("just clears the row when there is no stored reference", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { ...updatedMe, avatar: "" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useRemoveAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync(null);
    });
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
    expect(calls.filter((call) => call.method === "PATCH").at(-1)?.body).toEqual({ avatar: "" });
  });

  it("toasts a failed removal", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { error: "boom" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useRemoveAvatar(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ kind: "asset", assetId: "abc" }).catch(() => undefined);
    });
    expect(notifyError).toHaveBeenCalledWith(AVATAR_REMOVE_FAILED_TITLE, AVATAR_REMOVE_FAILED_MESSAGE);
  });
});

describe("email writes", () => {
  it("answers the taken check from the server field", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { existing: true, status: "CREDENTIAL" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useCheckEmailTaken(), { wrapper: Wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync("grace@example.com")).resolves.toBe(true);
    });
    expect(calls.find((call) => call.url.endsWith("/auth/email-check/"))?.body).toEqual({ email: "grace@example.com" });
  });

  it("asks for a mailed code", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { message: "Code sent" } }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useGenerateEmailCode(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync("grace@example.com");
    });
    expect(calls.find((call) => call.url.endsWith("/api/users/me/email/generate-code/"))?.body).toEqual({
      email: "grace@example.com",
    });
  });

  it("verifies the code, clears the CSRF cache and refreshes the row", async () => {
    const { transport, calls } = stubTransport(() => ({
      status: 200,
      body: {
        ...ME_ROW,
        email: "grace@example.com",
        username: "ada",
        cover_image: null,
        cover_image_url: null,
        date_joined: "2024-01-01T00:00:00Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "password",
        last_login_time: null,
      },
    }));
    const { queryClient, wrapper: Wrapper } = setup(transport);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useConfirmEmailChange(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ email: "grace@example.com", code: "123456" });
    });
    expect(calls.find((call) => call.method === "PATCH" && call.url.endsWith("/api/users/me/email/"))?.body).toEqual({
      email: "grace@example.com",
      code: "123456",
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sessionKeys.me });
  });
});

describe("useChangePassword", () => {
  it("posts, toasts success and refreshes the rows", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { message: "Password changed" } }));
    const { queryClient, wrapper: Wrapper } = setup(transport);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useChangePassword(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ oldPassword: "old", newPassword: "NewStr0ng!" });
    });
    expect(calls.find((call) => call.url.endsWith("/auth/change-password/"))?.body).toEqual({
      old_password: "old",
      new_password: "NewStr0ng!",
    });
    expect(notifySuccess).toHaveBeenCalledWith(PROFILE_SAVED_TITLE, "Password changed successfully.");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sessionKeys.me });
  });

  it("toasts the mapped refusal for a wrong current secret", async () => {
    const { transport } = stubTransport(() => ({
      status: 400,
      body: { error_code: "5135", error_message: "Incorrect old password" },
    }));
    const { wrapper: Wrapper } = setup(transport);
    const { result } = renderHook(() => useChangePassword(), { wrapper: Wrapper });
    await act(async () => {
      await result.current.mutateAsync({ oldPassword: "wrong", newPassword: "NewStr0ng!" }).catch(() => undefined);
    });
    expect(notifyError).toHaveBeenCalledWith("Incorrect old password", "Incorrect old password. Please try again.");
  });
});
