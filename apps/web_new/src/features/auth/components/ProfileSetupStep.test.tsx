// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import {
  AVATAR_UPLOAD_FAILED_MESSAGE,
  PROFILE_SAVE_FAILED_MESSAGE,
  PROFILE_SAVE_FAILED_TITLE,
  ProfileSetupStep,
} from "./ProfileSetupStep.js";

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

const ME_UUID = "123e4567-e89b-12d3-a456-426614174000";

function meFixture(overrides: Record<string, unknown> = {}) {
  return {
    id: ME_UUID,
    email: "ada@example.com",
    username: "ada",
    first_name: "",
    last_name: "",
    display_name: "",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2024-01-02T03:04:05Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "",
    last_login_time: null,
    ...overrides,
  };
}

function configFixture(selfManaged: boolean) {
  return {
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: false,
      is_email_password_enabled: true,
      is_smtp_configured: true,
      is_self_managed: selfManaged,
      instance_changelog_url: "",
    },
  };
}

// The flow gates on loading, so steps mount with the account cached: seed
// the cache synchronously instead of resolving it mid-render.
function seedSession(queryClient: QueryClient, me: Record<string, unknown>, selfManaged: boolean | "unread"): void {
  setSessionClient({
    getJson: async (url: string) => {
      if (url === "/api/users/me/") return me;
      if (url === "/api/instances/") {
        if (selfManaged === "unread") throw new Error("instance config unread");
        return configFixture(selfManaged);
      }
      throw new Error(`unexpected session read ${url}`);
    },
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  queryClient.setQueryData(["session", "me"], me);
  if (selfManaged !== "unread") {
    queryClient.setQueryData(["instance", "config"], configFixture(selfManaged).config);
  }
}

const PROFILE_BODY = {
  id: ME_UUID,
  user: ME_UUID,
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

interface MutationSetup {
  bodies: Record<string, string | undefined>;
  failUsersMe?: boolean;
}

function seedMutations(setup: MutationSetup): void {
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/") && init.method === "PATCH") {
        setup.bodies["users-me"] = init.body;
        if (setup.failUsersMe) return { status: 500, body: "{}", url };
        return {
          status: 200,
          body: JSON.stringify({
            id: ME_UUID,
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
      }
      if (url.endsWith("/api/users/me/profile/")) {
        setup.bodies["profile"] = init.body;
        return { status: 200, body: JSON.stringify(PROFILE_BODY), url };
      }
      if (url.endsWith("/auth/set-password/")) {
        setup.bodies["set-password"] = init.body;
        return {
          status: 200,
          body: JSON.stringify({ id: ME_UUID, email: "ada@example.com", is_password_autoset: false }),
          url,
        };
      }
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
}

function renderStep(
  options: {
    me?: Record<string, unknown>;
    selfManaged?: boolean | "unread";
    onAdvance?: () => void;
    bodies?: Record<string, string | undefined>;
    failUsersMe?: boolean;
  } = {}
): void {
  seedMutations({
    bodies: options.bodies ?? {},
    ...(options.failUsersMe === undefined ? {} : { failUsersMe: options.failUsersMe }),
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // Self-managed unless a test opts into the hosted backend (CLOUD-020);
  // "unread" leaves the config unseeded for the fail-closed paths.
  seedSession(queryClient, options.me ?? meFixture(), options.selfManaged ?? true);
  render(
    <QueryClientProvider client={queryClient}>
      <ProfileSetupStep onAdvance={options.onAdvance ?? (() => undefined)} />
    </QueryClientProvider>
  );
}

function continueButton(): HTMLElement {
  return screen.getByRole("button", { name: "Continue" });
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("ProfileSetupStep", () => {
  it("requires a valid name before continuing", async () => {
    const user = userEvent.setup();
    renderStep();

    expect(await screen.findByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
    expect(continueButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Your name"), "Ada Byron");
    expect(continueButton()).toBeEnabled();
  });

  it("shows the inline name error after blur", async () => {
    const user = userEvent.setup();
    renderStep();

    const name = await screen.findByLabelText("Your name");
    await user.type(name, "Ada123");
    expect(continueButton()).toBeDisabled();
    await user.tab();
    expect(screen.getByText("Use only letters, spaces, hyphens, and apostrophes.")).toBeInTheDocument();
  });

  it("saves the name and consent, then advances", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, onAdvance, selfManaged: false });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    await user.click(screen.getByLabelText("Email me product updates and tips"));
    await user.click(screen.getByLabelText("Email me product updates and tips"));
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(JSON.parse(bodies["users-me"] ?? "{}")).toEqual({ first_name: "Ada Byron" });
    await waitFor(() => expect(bodies["profile"]).toBeDefined());
    expect(JSON.parse(bodies["profile"] ?? "{}")).toEqual({ has_marketing_email_consent: true });
    expect(bodies["set-password"]).toBeUndefined();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("stores an unchecked consent choice", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    renderStep({ bodies, selfManaged: false });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    await user.click(screen.getByLabelText("Email me product updates and tips"));
    await user.click(continueButton());

    await waitFor(() => expect(bodies["profile"]).toBeDefined());
    expect(JSON.parse(bodies["profile"] ?? "{}")).toEqual({ has_marketing_email_consent: false });
  });

  it("offers no consent checkbox on self-managed instances", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, selfManaged: true, onAdvance });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    expect(screen.queryByLabelText("Email me product updates and tips")).toBeNull();
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(bodies["profile"]).toBeUndefined();
  });

  it("offers a password pair only to passwordless accounts", async () => {
    const user = userEvent.setup();
    renderStep({ me: meFixture({ is_password_autoset: true }) });

    const secret = await screen.findByLabelText("New password");
    const confirm = screen.getByLabelText("Confirm password");
    expect(continueButton()).toBeDisabled();

    await user.type(screen.getByLabelText("Your name"), "Ada Byron");
    await user.type(secret, "weak");
    expect(screen.getByText("Use at least 8 characters.")).toBeInTheDocument();
    await user.type(confirm, "different-secret");
    await user.tab();
    expect(screen.getByText("The two passwords do not match.")).toBeInTheDocument();
    expect(continueButton()).toBeDisabled();

    await user.clear(secret);
    await user.clear(confirm);
    await user.type(secret, "Tq7!mZ2@kP9#xR");
    await user.type(confirm, "Tq7!mZ2@kP9#xR");
    expect(continueButton()).toBeEnabled();
  });

  it("toggles password visibility", async () => {
    const user = userEvent.setup();
    renderStep({ me: meFixture({ is_password_autoset: true }) });

    await screen.findByLabelText("New password");
    const secret = screen.getByLabelText("New password");
    expect(secret).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Show new password" }));
    expect(secret).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "Hide new password" }));
    expect(secret).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Show confirmation" }));
    expect(screen.getByRole("button", { name: "Hide confirmation" })).toBeInTheDocument();
  });

  it("sets the first password on submit", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, me: meFixture({ is_password_autoset: true }), onAdvance });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    await user.type(screen.getByLabelText("New password"), "Tq7!mZ2@kP9#xR");
    await user.type(screen.getByLabelText("Confirm password"), "Tq7!mZ2@kP9#xR");
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(JSON.parse(bodies["set-password"] ?? "{}")).toEqual({ password: "Tq7!mZ2@kP9#xR" });
  });

  it("asks for no password on the hosted backend", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, me: meFixture({ is_password_autoset: true }), selfManaged: false, onAdvance });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    expect(screen.queryByLabelText("New password")).toBeNull();
    expect(screen.queryByLabelText("Confirm password")).toBeNull();
    expect(document.querySelectorAll('input[type="password"]')).toHaveLength(0);
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(bodies["set-password"]).toBeUndefined();
    expect(JSON.parse(bodies["users-me"] ?? "{}")).toEqual({ first_name: "Ada Byron" });
  });

  it("keeps the password pair when the instance config is unread", async () => {
    renderStep({ me: meFixture({ is_password_autoset: true }), selfManaged: "unread" });
    expect(await screen.findByLabelText("New password")).toBeVisible();
  });

  it("toasts but still advances when the save fails", async () => {
    const user = userEvent.setup();
    const onAdvance = vi.fn();
    renderStep({ onAdvance, failUsersMe: true });

    await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(notifyError).toHaveBeenCalledWith(PROFILE_SAVE_FAILED_TITLE, PROFILE_SAVE_FAILED_MESSAGE);
  });

  it("uploads an avatar and stores its URL", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: true, status: 204 })) as unknown as typeof fetch;
    const onAdvance = vi.fn();
    try {
      renderStep({ bodies, onAdvance });
      await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
      await user.click(screen.getByRole("button", { name: "Add a photo" }));

      const picker = screen.getByLabelText("Choose an image");
      await user.upload(picker, new File(["bytes"], "ada.png", { type: "image/png" }));
      expect(screen.getByText("ada.png (5 bytes)")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Upload" }));

      await waitFor(() => expect(screen.getByRole("button", { name: "Change photo" })).toBeInTheDocument());
      await user.click(continueButton());
      await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
      expect(JSON.parse(bodies["users-me"] ?? "{}")).toEqual({
        first_name: "Ada Byron",
        avatar: "https://cdn.test/avatars/asset-1.png",
      });
    } finally {
      globalThis.fetch = realFetch;
    }
  });

  it("rejects invalid avatar files inline", async () => {
    const user = userEvent.setup();
    renderStep();

    await screen.findByLabelText("Your name");
    await user.click(screen.getByRole("button", { name: "Add a photo" }));
    // fireEvent: user-event's upload enforces the input's accept list, so
    // rejected files arrive through the raw change path instead.
    fireEvent.change(screen.getByLabelText("Choose an image"), {
      target: { files: [new File(["bytes"], "ada.gif", { type: "image/gif" })] },
    });
    expect(screen.getByText("Choose a JPEG, PNG, or WebP image.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Choose an image"), { target: { files: [] } });
    await user.upload(
      screen.getByLabelText("Choose an image"),
      new File([new Uint8Array(5 * 1024 * 1024 + 1)], "big.png", { type: "image/png" })
    );
    expect(screen.getByText("Choose an image under 5 MB.")).toBeInTheDocument();
  });

  it("clears the chosen file when the picker reports no files", async () => {
    const user = userEvent.setup();
    renderStep();

    await screen.findByLabelText("Your name");
    await user.click(screen.getByRole("button", { name: "Add a photo" }));
    await user.upload(screen.getByLabelText("Choose an image"), new File(["bytes"], "ada.png", { type: "image/png" }));
    expect(screen.getByText("ada.png (5 bytes)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload" })).toBeEnabled();

    fireEvent.change(screen.getByLabelText("Choose an image"), { target: { files: null } });
    expect(screen.queryByText("ada.png (5 bytes)")).toBeNull();
    expect(screen.getByRole("button", { name: "Upload" })).toBeDisabled();
  });

  it("disables upload with no file chosen", () => {
    renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Add a photo" }));
    expect(screen.getByRole("button", { name: "Upload" })).toBeDisabled();
  });

  it("reports a failed avatar upload inline", async () => {
    const user = userEvent.setup();
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: false, status: 403 })) as unknown as typeof fetch;
    try {
      renderStep();
      await screen.findByLabelText("Your name");
      await user.click(screen.getByRole("button", { name: "Add a photo" }));
      await user.upload(
        screen.getByLabelText("Choose an image"),
        new File(["bytes"], "ada.png", { type: "image/png" })
      );
      await user.click(screen.getByRole("button", { name: "Upload" }));
      expect(await screen.findByText(AVATAR_UPLOAD_FAILED_MESSAGE)).toBeInTheDocument();
    } finally {
      globalThis.fetch = realFetch;
    }
  });

  it("removes a stored avatar", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, me: meFixture({ avatar_url: "https://cdn.test/old.png", first_name: "Ada" }), onAdvance });

    await screen.findByLabelText("Your name");
    await user.click(screen.getByRole("button", { name: "Change photo" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(screen.getByRole("button", { name: "Add a photo" })).toBeInTheDocument();
    await user.click(continueButton());

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(JSON.parse(bodies["users-me"] ?? "{}")).toEqual({ first_name: "Ada", avatar: "" });
  });

  it("disables submit while an avatar upload is in flight", async () => {
    const user = userEvent.setup();
    const realFetch = globalThis.fetch;
    let release!: (value: { ok: boolean; status: number }) => void;
    const gate = new Promise<{ ok: boolean; status: number }>((resolve) => {
      release = resolve;
    });
    globalThis.fetch = (() => gate) as unknown as typeof fetch;
    try {
      renderStep();
      await user.type(await screen.findByLabelText("Your name"), "Ada Byron");
      await user.click(screen.getByRole("button", { name: "Add a photo" }));
      await user.upload(
        screen.getByLabelText("Choose an image"),
        new File(["bytes"], "ada.png", { type: "image/png" })
      );
      await user.click(screen.getByRole("button", { name: "Upload" }));
      // The open dialog hides the background form from accessible queries.
      const hiddenContinue = () => screen.getByRole("button", { name: "Continue", hidden: true });
      await waitFor(() => expect(hiddenContinue()).toBeDisabled());
      await act(async () => {
        release({ ok: true, status: 204 });
      });
      await waitFor(() => expect(continueButton()).toBeEnabled());
    } finally {
      globalThis.fetch = realFetch;
    }
  });

  it("renders fallbacks while the account loads", () => {
    seedMutations({ bodies: {} });
    setSessionClient({
      getJson: () => new Promise(() => undefined),
      parse: (_schema: unknown, data: unknown) => data,
    } as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <ProfileSetupStep onAdvance={() => undefined} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
    expect(screen.getByLabelText("Your name")).toHaveValue("");
    expect(screen.queryByLabelText("New password")).toBeNull();
    expect(continueButton()).toBeDisabled();
  });
});
