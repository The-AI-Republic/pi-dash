// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract unit tests: each contract parses a representative payload through
// a stub transport. Live-server proof lives in contract/*.contract.test.ts.
import { describe, expect, it } from "vitest";
import { ApiError } from "./errors.js";
import { createClient } from "./client.js";
import {
  approveDeviceCode,
  checkEmail,
  deactivateAccount,
  generateMagicCode,
  getCsrfToken,
  getInstanceConfig,
  parseAuthRedirectResult,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signInWithMagicCode,
  signOut,
  signUp,
  signUpWithMagicCode,
} from "./contracts/auth.js";
import { getMe, getMeSettings } from "./contracts/users.js";
import { listWorkspaces } from "./contracts/workspaces.js";
import {
  addFavoriteProject,
  archiveProject,
  createProject,
  deleteProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  joinProjects,
  leaveProject,
  listFavoriteProjects,
  listProjectDetails,
  listProjectInvitations,
  listProjects,
  removeFavoriteProject,
  unarchiveProject,
  updateProjectUserProperties,
  updateWorkspaceUserProperties,
} from "./contracts/projects.js";
import {
  completeTour,
  createQuickLink,
  deleteQuickLink,
  getQuickLink,
  getWorkspaceDashboard,
  listHomePreferences,
  listQuickLinks,
  listRecentVisits,
  updateHomePreference,
  updateQuickLink,
} from "./contracts/home.js";
import { listLabels, listMembers, listStates } from "./contracts/reference.js";
import {
  getIssue,
  getSubscriptionStatus,
  listIssues,
  subscribeIssue,
  unsubscribeIssue,
  updateIssue,
} from "./contracts/issues.js";
import {
  createFavorite,
  deleteFavorite,
  getSidebarPreferences,
  listFavoriteGroup,
  listFavorites,
  updateFavorite,
  updateSidebarPreferences,
} from "./contracts/sidebar.js";
import { searchWorkspace } from "./contracts/search.js";
import { getProfile, listTimezones, updateMyTimezone, updateProfile } from "./contracts/profile.js";
import { createAssistantThread, getLlmConfig, listAssistantThreads } from "./contracts/assistant.js";
import { getUnreadNotifications } from "./contracts/notifications.js";
import { updateCycle } from "./contracts/cycles.js";
import { updateModule } from "./contracts/modules.js";
import { updatePage } from "./contracts/pages.js";
import { fakeResponse, recordingTransport } from "./client.test.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function json(value: unknown) {
  return recordingTransport(() => fakeResponse({ body: JSON.stringify(value), url: "https://api.test/" }));
}

describe("auth contracts", () => {
  it("parses the CSRF response", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ csrf_token: "tok" }) });
    await expect(getCsrfToken(client)).resolves.toEqual({ csrf_token: "tok" });
  });

  it("rejects a CSRF response without a token", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({}) });
    const error = await getCsrfToken(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("reads sign-in success from the landing URL", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signIn(client, { email: "a@b.c", password: "secret" });
    expect(result).toEqual({ ok: true, location: "https://app.test/" });
    const signInCall = transport.calls.find((call) => call.url.endsWith("/auth/sign-in/"));
    expect(signInCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(signInCall?.init.headers["X-CSRFTOKEN"]).toBeDefined();
  });

  it("reads sign-in failure from error query params", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            status: 200,
            body: "<html>",
            url: "https://app.test/sign-in?error_code=USER_DOES_NOT_EXIST&error_message=USER_DOES_NOT_EXIST",
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signIn(client, { email: "a@b.c", password: "nope" });
    expect(result).toEqual({
      ok: false,
      location: "https://app.test/sign-in?error_code=USER_DOES_NOT_EXIST&error_message=USER_DOES_NOT_EXIST",
      code: "USER_DOES_NOT_EXIST",
      message: "USER_DOES_NOT_EXIST",
    });
  });

  it("routes the email check to the password or code step", async () => {
    const seen: string[] = [];
    const transport = recordingTransport((call) => {
      seen.push(call.url);
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse({ body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkEmail(client, "ada@example.com");
    expect(parsed).toEqual({ existing: true, status: "MAGIC_CODE" });
    const checkCall = transport.calls.find((call) => call.url.endsWith("/auth/email-check/"));
    expect(checkCall?.init.method).toBe("POST");
    expect(checkCall?.init.body).toBe(JSON.stringify({ email: "ada@example.com" }));
    expect(seen.some((url) => url.endsWith("/auth/get-csrf-token/"))).toBe(true);
  });

  it("rejects an email-check answer with an unknown mode", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ existing: false, status: "OAUTH" }) });
    const error = await checkEmail(client, "new@example.com").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses the magic-generate key", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify({ key: "magic_ada@example.com" }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await generateMagicCode(client, "ada@example.com");
    expect(parsed).toEqual({ key: "magic_ada@example.com" });
    expect(transport.calls.some((call) => call.url.endsWith("/auth/magic-generate/"))).toBe(true);
  });

  it("reads magic sign-in from the same redirect convention", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/acme" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signInWithMagicCode(client, { email: "ada@example.com", code: "123456" });
    expect(result).toEqual({ ok: true, location: "https://app.test/acme" });
    const codeCall = transport.calls.find((call) => call.url.endsWith("/auth/magic-sign-in/"));
    expect(codeCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(codeCall?.init.body).toContain("code=123456");
  });

  it("reads magic sign-in failure from error query params", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            status: 200,
            body: "<html>",
            url: "https://app.test/sign-in?error_code=INVALID_MAGIC_CODE_SIGN_IN&error_message=INVALID_MAGIC_CODE_SIGN_IN",
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signInWithMagicCode(client, { email: "ada@example.com", code: "000000" });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe("INVALID_MAGIC_CODE_SIGN_IN");
  });

  it("posts sign-out and reports the landing URL", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/sign-in" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signOut(client);
    expect(result).toEqual({ location: "https://app.test/sign-in" });
    expect(transport.calls.some((call) => call.url.endsWith("/auth/sign-out/"))).toBe(true);
  });

  it("refetches the CSRF token after session-changing flows", async () => {
    // Django rotates the CSRF secret on login; the client must not reuse
    // the pre-login token afterwards (the server answers a bespoke 200
    // failure page instead of a 403, so reuse fails silently).
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/acme" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await signIn(client, { email: "a@b.c", password: "secret" });
    await signOut(client);
    const csrfCalls = transport.calls.filter((call) => call.url.endsWith("/auth/get-csrf-token/"));
    expect(csrfCalls).toHaveLength(2);
  });

  it("posts native sign-up and reads the landing URL", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/onboarding" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signUp(client, {
      email: "new@example.com",
      password: "secret",
      confirmPassword: "secret",
      nextPath: "/invites",
    });
    expect(result).toEqual({ ok: true, location: "https://app.test/onboarding" });
    const signUpCall = transport.calls.find((call) => call.url.endsWith("/auth/sign-up/"));
    expect(signUpCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(signUpCall?.init.body).toContain("confirm_password=secret");
    expect(signUpCall?.init.body).toContain(`next_path=${encodeURIComponent("/invites")}`);
  });

  it("reads sign-up failure from numeric error query params", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            status: 200,
            body: "<html>",
            url: "https://app.test/?error_code=5030&error_message=USER_ALREADY_EXIST",
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signUp(client, { email: "ada@example.com", password: "secret" });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe("5030");
      expect(result.message).toBe("USER_ALREADY_EXIST");
    }
  });

  it("posts code sign-up through the same redirect convention", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/onboarding" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await signUpWithMagicCode(client, { email: "new@example.com", code: "123456" });
    expect(result).toEqual({ ok: true, location: "https://app.test/onboarding" });
    const codeCall = transport.calls.find((call) => call.url.endsWith("/auth/magic-sign-up/"));
    expect(codeCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(codeCall?.init.body).toContain("code=123456");
  });

  it("requests a password reset and parses the confirmation", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify({ message: "Check your email to reset your password" }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await requestPasswordReset(client, "ada@example.com");
    expect(parsed).toEqual({ message: "Check your email to reset your password" });
    const resetCall = transport.calls.find((call) => call.url.endsWith("/auth/forgot-password/"));
    expect(resetCall?.init.method).toBe("POST");
    expect(resetCall?.init.body).toBe(JSON.stringify({ email: "ada@example.com" }));
  });

  it("rejects a reset request without a confirmation message", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({}) });
    const error = await requestPasswordReset(client, "ada@example.com").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("posts the reset to the uid/token URL", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            status: 200,
            body: "<html>",
            url: "https://app.test/accounts/reset-password?error_code=5125&error_message=INVALID_PASSWORD_TOKEN",
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await resetPassword(client, {
      uid: "dWlk",
      token: "tok-en_1",
      password: "secret",
      confirmPassword: "secret",
    });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe("5125");
    const resetCall = transport.calls.find((call) => call.url.includes("/auth/reset-password/"));
    expect(resetCall?.url).toBe("https://api.test/auth/reset-password/dWlk/tok-en_1/");
    expect(resetCall?.init.body).toContain("password=secret");
    expect(resetCall?.init.body).toContain("confirm_password=secret");
  });

  it("passes optional fields through the code and reset paths", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/acme" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await signUpWithMagicCode(client, { email: "new@example.com", code: "123456", nextPath: "/invites" });
    const codeCall = transport.calls.find((call) => call.url.endsWith("/auth/magic-sign-up/"));
    expect(codeCall?.init.body).toContain(`next_path=${encodeURIComponent("/invites")}`);
    const result = await resetPassword(client, { uid: "dWlk", token: "tok-en_1", password: "secret" });
    expect(result).toEqual({ ok: true, location: "https://app.test/acme" });
    const resetCall = transport.calls.find((call) => call.url.includes("/auth/reset-password/"));
    const resetBody = resetCall?.init.body;
    expect(typeof resetBody).toBe("string");
    expect(resetBody ?? "").not.toContain("confirm_password");
  });

  it("sets the first password and reads the user projection", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            body: JSON.stringify({
              id: UUID,
              email: "ada@example.com",
              is_password_autoset: false,
              first_name: "Ada",
            }),
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await setPassword(client, "fresh-secret");
    expect(parsed).toEqual({ id: UUID, email: "ada@example.com", is_password_autoset: false });
    const setCall = transport.calls.find((call) => call.url.endsWith("/auth/set-password/"));
    expect(setCall?.init.body).toBe(JSON.stringify({ password: "fresh-secret" }));
  });

  it("approves a device code and parses the grant", async () => {
    const grant = { ok: true, user_email: "ada@example.com", workspace_slug: "acme" };
    const client = createClient({ baseUrl: "https://api.test", transport: json(grant) });
    const parsed = await approveDeviceCode(client, "ABCD-1234");
    expect(parsed).toEqual(grant);
  });

  it("parses a device grant without a workspace", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ ok: true, user_email: "ada@example.com", workspace_slug: null }),
    });
    const parsed = await approveDeviceCode(client, "ABCD-1234");
    expect(parsed.workspace_slug).toBeNull();
  });

  it("rejects a device grant that is not ok", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ ok: false, user_email: "ada@example.com", workspace_slug: null }),
    });
    const error = await approveDeviceCode(client, "ABCD-1234").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("deactivates the account and reports the status", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 204, body: "" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const status = await deactivateAccount(client);
    expect(status).toBe(204);
    const deleteCall = transport.calls.find((call) => call.url.endsWith("/api/users/me/"));
    expect(deleteCall?.init.method).toBe("DELETE");
  });

  it("reads the instance config and strips the rest", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({
        config: {
          enable_signup: false,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: true,
          is_email_password_enabled: true,
          is_smtp_configured: true,
          is_self_managed: true,
          github_app_name: "",
          file_size_limit: 5242880,
        },
        instance: { id: UUID, workspaces_exist: true },
      }),
    });
    const parsed = await getInstanceConfig(client);
    expect(parsed).toEqual({
      enable_signup: false,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: true,
      is_email_password_enabled: true,
      is_smtp_configured: true,
      is_self_managed: true,
    });
  });

  it("rejects an instance answer without the config flags", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ config: { enable_signup: true } }),
    });
    const error = await getInstanceConfig(client).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses link-carried auth errors off a URL", () => {
    expect(parseAuthRedirectResult("https://app.test/acme")).toEqual({
      ok: true,
      location: "https://app.test/acme",
    });
    expect(parseAuthRedirectResult("")).toEqual({ ok: true, location: "" });
    expect(parseAuthRedirectResult("https://app.test/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED")).toEqual({
      ok: false,
      location: "https://app.test/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED",
      code: "5900",
      message: "RATE_LIMIT_EXCEEDED",
    });
    const bare = parseAuthRedirectResult("https://app.test/?error_code=5060");
    expect(bare.ok).toBe(false);
    if (!bare.ok) expect(bare.message).toBe("5060");
  });

  it("refetches the CSRF token after the new session-changing writes", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 200, body: "<html>", url: "https://app.test/acme" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await signUp(client, { email: "new@example.com", password: "secret" });
    await signUpWithMagicCode(client, { email: "new@example.com", code: "123456" });
    const csrfCalls = transport.calls.filter((call) => call.url.endsWith("/auth/get-csrf-token/"));
    expect(csrfCalls).toHaveLength(2);
  });
});

describe("users contracts", () => {
  const me = {
    id: UUID,
    email: "ada@example.com",
    username: "ada",
    first_name: "Ada",
    last_name: "Lovelace",
    display_name: "Ada Lovelace",
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
    last_login_medium: "email",
    last_login_time: null,
    extra_future_field: "ignored",
  };

  it("parses me and tolerates unknown fields", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(me) });
    const parsed = await getMe(client);
    expect(parsed.email).toBe("ada@example.com");
    expect(parsed).not.toHaveProperty("extra_future_field");
  });

  it("rejects me without an id", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ ...me, id: "nope" }) });
    const error = await getMe(client).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses me settings with workspace pointers", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({
        id: UUID,
        email: "ada@example.com",
        workspace: {
          last_workspace_id: UUID,
          last_workspace_slug: "acme",
          fallback_workspace_id: UUID,
          fallback_workspace_slug: "acme",
          invites: 0,
        },
      }),
    });
    const parsed = await getMeSettings(client);
    expect(parsed.workspace.invites).toBe(0);
  });
});

describe("workspace contracts", () => {
  it("parses the workspace list", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([{ id: UUID, name: "Acme", slug: "acme", logo_url: "", total_members: 3, role: 20 }]),
    });
    const parsed = await listWorkspaces(client);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.role).toBe(20);
  });
});

describe("project contracts", () => {
  const project = {
    id: UUID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID2,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };

  it("parses the project list", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([project]) });
    const parsed = await listProjects(client, "acme");
    expect(parsed[0]?.identifier).toBe("WEB");
    expect(parsed[0]?.member_role).toBe(20);
  });

  it("hits the workspace-scoped URL", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: "[]" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await listProjects(client, "acme");
    expect(transport.calls[0]?.url).toBe("https://api.test/api/workspaces/acme/projects/");
  });
});

describe("reference contracts", () => {
  it("parses states", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "Todo",
          color: "#ff0000",
          group: "unstarted",
          default: true,
          description: "",
          sequence: 65535,
        },
      ]),
    });
    const parsed = await listStates(client, "acme", UUID2);
    expect(parsed[0]?.group).toBe("unstarted");
  });

  it("rejects unknown state groups", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "X",
          color: "#fff",
          group: "warp",
          default: false,
          description: "",
          sequence: 1,
        },
      ]),
    });
    const error = await listStates(client, "acme", UUID2).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("parses labels", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([
        {
          id: UUID,
          project_id: UUID2,
          workspace_id: UUID2,
          name: "bug",
          color: "#e11d48",
          parent: null,
          sort_order: 1,
        },
      ]),
    });
    const parsed = await listLabels(client, "acme", UUID2);
    expect(parsed[0]?.parent).toBeNull();
  });

  it("parses members as role projections", async () => {
    const member = {
      id: UUID,
      role: 20,
      member: UUID,
      project: UUID2,
      original_role: 20,
      created_at: "2024-01-02T03:04:05Z",
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json([member]) });
    const parsed = await listMembers(client, "acme", UUID2);
    expect(parsed[0]?.role).toBe(20);
    expect(parsed[0]?.member).toBe(UUID);
  });
});

describe("issue contracts", () => {
  const issue = {
    id: UUID,
    name: "Fix login",
    state_id: UUID2,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 42,
    project_id: UUID2,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: null,
    link_count: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };

  const issueDetail = {
    ...issue,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };

  const envelope = {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 1,
    next_cursor: "1:1:0",
    prev_cursor: "1:0:0",
    next_page_results: false,
    prev_page_results: false,
    count: 1,
    total_pages: 1,
    total_results: 1,
    extra_stats: null,
    results: [issue],
  };

  it("parses the ungrouped list envelope", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(envelope) });
    const parsed = await listIssues(client, "acme", UUID2);
    expect(parsed.total_count).toBe(1);
    expect(parsed.results[0]?.sequence_id).toBe(42);
  });

  it("parses issue detail", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(issueDetail) });
    const parsed = await getIssue(client, "acme", UUID2, UUID);
    expect(parsed.name).toBe("Fix login");
    expect(parsed.has_open_blockers).toBe(false);
  });

  it("hits the detail URL", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(issueDetail) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await getIssue(client, "acme", UUID2, UUID);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID2}/issues/${UUID}/`);
  });

  it("sends the issue update and answers nothing (204)", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 204, body: "" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(updateIssue(client, "acme", UUID2, UUID, { name: "Renamed" })).resolves.toBeUndefined();
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID2}/issues/${UUID}/`);
    expect(patchCall?.init.body).toBe(JSON.stringify({ name: "Renamed" }));
  });

  it("reads and flips the subscription", async () => {
    let subscribed = false;
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "GET") return fakeResponse({ body: JSON.stringify({ subscribed }) });
      if (call.init.method === "POST") {
        subscribed = true;
        return fakeResponse({ status: 201, body: JSON.stringify({ id: UUID, subscriber: UUID2 }) });
      }
      subscribed = false;
      return fakeResponse({ status: 204, body: "" });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    expect((await getSubscriptionStatus(client, "acme", UUID2, UUID)).subscribed).toBe(false);
    const subscription = await subscribeIssue(client, "acme", UUID2, UUID);
    expect(subscription.subscriber).toBe(UUID2);
    expect((await getSubscriptionStatus(client, "acme", UUID2, UUID)).subscribed).toBe(true);
    await unsubscribeIssue(client, "acme", UUID2, UUID);
    expect((await getSubscriptionStatus(client, "acme", UUID2, UUID)).subscribed).toBe(false);
  });
});

describe("sidebar contracts", () => {
  it("parses the preferences map and patches it back", async () => {
    const prefs = { drafts: { is_pinned: true, sort_order: 65535 }, analytics: { is_pinned: false, sort_order: 1 } };
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : call.init.method === "PATCH"
          ? fakeResponse({ body: JSON.stringify({ message: "Successfully updated" }) })
          : fakeResponse({ body: JSON.stringify(prefs) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await getSidebarPreferences(client, "acme");
    expect(parsed["drafts"]?.is_pinned).toBe(true);
    const patched = await updateSidebarPreferences(client, "acme", [{ key: "analytics", is_pinned: true }]);
    expect(patched.message).toBe("Successfully updated");
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe("https://api.test/api/workspaces/acme/sidebar-preferences/");
    expect(patchCall?.init.body).toBe(JSON.stringify([{ key: "analytics", is_pinned: true }]));
  });

  it("rejects a preferences map with a malformed row", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ drafts: { is_pinned: "yes", sort_order: 1 } }),
    });
    const error = await getSidebarPreferences(client, "acme").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("round-trips favorites through create, group, move and delete", async () => {
    const folder = {
      id: UUID,
      entity_type: "folder",
      entity_identifier: null,
      entity_data: null,
      name: "Folder",
      is_folder: true,
      sequence: 65535,
      parent: null,
      workspace_id: UUID2,
      project_id: null,
    };
    const entry = { ...folder, id: UUID2, entity_type: "issue", entity_identifier: UUID, is_folder: false };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "GET" && call.url.endsWith("/user-favorites/")) {
        return fakeResponse({ body: JSON.stringify([entry]) });
      }
      if (call.init.method === "POST") return fakeResponse({ body: JSON.stringify(entry) });
      if (call.init.method === "PATCH") {
        return fakeResponse({ body: JSON.stringify({ ...entry, parent: null, sequence: 12.5 }) });
      }
      if (call.url.endsWith("/group/")) return fakeResponse({ body: JSON.stringify([entry]) });
      return fakeResponse({ status: 204, body: "" });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    expect((await listFavorites(client, "acme"))[0]?.entity_type).toBe("issue");
    const created = await createFavorite(client, "acme", { entity_type: "issue", entity_identifier: UUID });
    expect(created.entity_data).toBeNull();
    const children = await listFavoriteGroup(client, "acme", UUID);
    expect(children).toHaveLength(1);
    const moved = await updateFavorite(client, "acme", UUID2, { parent: null, sequence: 12.5 });
    expect(moved.sequence).toBe(12.5);
    await deleteFavorite(client, "acme", UUID2);
    const deleteCall = transport.calls.find((call) => call.init.method === "DELETE");
    expect(deleteCall?.url).toBe(`https://api.test/api/workspaces/acme/user-favorites/${UUID2}/`);
  });
});

describe("search contracts", () => {
  it("parses per-entity results and sends the query params", async () => {
    const payload = {
      results: {
        issue: [
          {
            name: "Fix login",
            id: UUID,
            sequence_id: 42,
            project__identifier: "WEB",
            project_id: UUID2,
            workspace__slug: "acme",
          },
        ],
      },
    };
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(payload) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await searchWorkspace(client, "acme", {
      search: "login",
      workspaceSearch: true,
      entities: ["issue"],
    });
    expect(parsed.results.issue?.[0]?.sequence_id).toBe(42);
    const url = transport.calls[0]?.url ?? "";
    expect(url).toContain("/api/workspaces/acme/search/");
    expect(url).toContain("search=login");
    expect(url).toContain("workspace_search=true");
    expect(url).toContain("entities=issue");
  });
});

describe("profile contracts", () => {
  const profile = {
    id: UUID,
    user: UUID2,
    theme: { mode: "dark" },
    is_app_rail_docked: true,
    is_tour_completed: false,
    onboarding_step: {},
    use_case: null,
    role: null,
    is_onboarded: false,
    last_workspace_id: null,
    billing_address_country: "INDIA",
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
    background_color: "#fff",
    is_navigation_tour_completed: false,
    has_marketing_email_consent: false,
    is_subscribed_to_changelog: false,
    product_tour: {},
    settings: {},
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };

  it("parses the profile and patches theme plus language", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            body: JSON.stringify(call.init.method === "PATCH" ? { ...profile, language: "fr" } : profile),
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    expect((await getProfile(client)).language).toBe("en");
    const patched = await updateProfile(client, { language: "fr", theme: { mode: "light" } });
    expect(patched.language).toBe("fr");
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe("https://api.test/api/users/me/profile/");
  });

  it("parses the timezone patch and the zone list", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.url.endsWith("/api/timezones/")) {
        return fakeResponse({
          body: JSON.stringify({
            timezones: [{ utc_offset: "UTC+01:00", gmt_offset: "GMT+01:00", value: "Europe/Paris", label: "Paris" }],
          }),
        });
      }
      return fakeResponse({
        body: JSON.stringify({ id: UUID, email: "ada@example.com", user_timezone: "Europe/Paris" }),
      });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const updated = await updateMyTimezone(client, "Europe/Paris");
    expect(updated.user_timezone).toBe("Europe/Paris");
    const zones = await listTimezones(client);
    expect(zones.timezones[0]?.value).toBe("Europe/Paris");
  });
});

describe("assistant contracts", () => {
  it("parses threads and the model-config check", async () => {
    const thread = {
      id: UUID,
      title: "Hello",
      is_archived: false,
      has_active_turn: false,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
    };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "POST") return fakeResponse({ status: 201, body: JSON.stringify(thread) });
      if (call.url.endsWith("/ai-assistant/config/")) {
        return fakeResponse({
          body: JSON.stringify({
            provider_kind: "openai_compatible",
            base_url: "",
            model_name: "",
            has_api_key: false,
            last_verified_at: null,
          }),
        });
      }
      return fakeResponse({ body: JSON.stringify([thread]) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const threads = await listAssistantThreads(client, "acme");
    expect(threads[0]?.title).toBe("Hello");
    const created = await createAssistantThread(client, "acme", "Hello");
    expect(created.id).toBe(UUID);
    const config = await getLlmConfig(client);
    expect(config.has_api_key).toBe(false);
  });
});

describe("notification contracts", () => {
  it("parses the unread counts", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ total_unread_notifications_count: 3, mention_unread_notifications_count: 1 }),
    });
    const parsed = await getUnreadNotifications(client, "acme");
    expect(parsed.total_unread_notifications_count).toBe(3);
    expect(parsed.mention_unread_notifications_count).toBe(1);
  });
});

describe("thin entity update contracts", () => {
  it("parses cycle, module and page update echoes", async () => {
    const cycle = {
      id: UUID,
      name: "Cycle",
      project_id: UUID2,
      description: "",
      start_date: null,
      end_date: null,
    };
    const module = {
      id: UUID,
      name: "Module",
      project_id: UUID2,
      description: "",
      start_date: null,
      target_date: null,
    };
    const page = { id: UUID, name: "Page", access: 0, color: "", is_locked: false, project_ids: [UUID2] };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.url.includes("/cycles/")) return fakeResponse({ body: JSON.stringify(cycle) });
      if (call.url.includes("/modules/")) return fakeResponse({ body: JSON.stringify(module) });
      return fakeResponse({ body: JSON.stringify(page) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    expect((await updateCycle(client, "acme", UUID2, UUID, { name: "Cycle" })).project_id).toBe(UUID2);
    expect((await updateModule(client, "acme", UUID2, UUID, { name: "Module" })).name).toBe("Module");
    expect((await updatePage(client, "acme", UUID2, UUID, { name: "Page" })).project_ids).toEqual([UUID2]);
  });
});

describe("home contracts", () => {
  const dashboard = {
    issue_activities: [{ created_date: "2026-10-01", activity_count: 3 }],
    completed_issues: [{ week_in_month: 1, completed_count: 2 }],
    assigned_issues_count: 5,
    pending_issues_count: 4,
    completed_issues_count: 1,
    issues_due_week_count: 0,
    state_distribution: [{ state_group: "started", state_count: 4 }],
    overdue_issues: [
      {
        id: UUID,
        name: "Late",
        workspace__slug: "acme",
        project_id: UUID2,
        target_date: "2026-09-01T00:00:00Z",
      },
    ],
    upcoming_issues: [
      {
        id: UUID,
        name: "Soon",
        workspace__slug: "acme",
        project_id: UUID2,
        start_date: null,
      },
    ],
  };

  it("parses the workspace dashboard", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(dashboard) });
    const parsed = await getWorkspaceDashboard(client, "acme");
    expect(parsed.assigned_issues_count).toBe(5);
    expect(parsed.overdue_issues[0]?.name).toBe("Late");
  });

  it("sends the dashboard month filter as a query param", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(dashboard) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await getWorkspaceDashboard(client, "acme", 10);
    expect(transport.calls[0]?.url).toContain("month=10");
  });

  const preferences = [
    { key: "quick_links", is_enabled: true, config: {}, sort_order: 999 },
    { key: "recents", is_enabled: false, config: {}, sort_order: 998 },
  ];

  it("parses home preferences and patches one key", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            body: JSON.stringify(
              call.init.method === "PATCH" ? { key: "recents", is_enabled: true, sort_order: 998 } : preferences
            ),
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const listed = await listHomePreferences(client, "acme");
    expect(listed).toHaveLength(2);
    const updated = await updateHomePreference(client, "acme", "recents", { is_enabled: true });
    expect(updated).toEqual({ key: "recents", is_enabled: true, sort_order: 998 });
    expect(transport.calls.some((call) => call.url.endsWith("/home-preferences/recents/"))).toBe(true);
  });

  const link = {
    id: UUID,
    workspace: UUID2,
    owner: UUID,
    title: "Docs",
    url: "https://example.com/docs",
    metadata: {},
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };

  it("runs the quick-link CRUD round trip", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "DELETE") return fakeResponse({ status: 204, body: "" });
      if (call.init.method === "PATCH") {
        return fakeResponse({ body: JSON.stringify({ ...link, title: "Docs renamed" }) });
      }
      if (call.init.method === "POST" || call.url.endsWith("/quick-links/")) {
        return fakeResponse({ body: JSON.stringify(call.init.method === "GET" ? [link] : link) });
      }
      return fakeResponse({ body: JSON.stringify(link) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const created = await createQuickLink(client, "acme", { title: "Docs", url: "https://example.com/docs" });
    expect(created.id).toBe(UUID);
    await expect(getQuickLink(client, "acme", UUID)).resolves.toMatchObject({ url: "https://example.com/docs" });
    const updated = await updateQuickLink(client, "acme", UUID, { title: "Docs renamed" });
    expect(updated.title).toBe("Docs renamed");
    const listed = await listQuickLinks(client, "acme");
    expect(listed).toHaveLength(1);
    const createCall = transport.calls.find((call) => call.url.endsWith("/quick-links/"));
    expect(JSON.parse((createCall?.init.body as string) ?? "{}")).toEqual({
      title: "Docs",
      url: "https://example.com/docs",
    });
    await expect(deleteQuickLink(client, "acme", UUID)).resolves.toBe(204);
  });

  it("lists recent visits with an optional entity filter", async () => {
    const visits = [
      {
        id: UUID,
        entity_name: "issue",
        entity_identifier: UUID2,
        entity_data: null,
        visited_at: "2026-10-01T00:00:00Z",
      },
    ];
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(visits) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await listRecentVisits(client, "acme", "issue");
    expect(parsed).toHaveLength(1);
    expect(transport.calls[0]?.url).toContain("entity_name=issue");
  });

  it("marks the tour complete", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify({ message: "Updated successfully" }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(completeTour(client, { is_tour_completed: true })).resolves.toEqual({
      message: "Updated successfully",
    });
  });
});

describe("project lifecycle contracts", () => {
  const details = {
    id: UUID2,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: UUID,
    updated_by: null,
    deleted_at: null,
    workspace: UUID,
    name: "Contract",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "CT",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: true,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 20,
    anchor: null,
    members: [UUID],
    next_work_item_sequence: 3,
    agent_executor_options: [],
  };

  function stubbedApi(
    handler: (call: { url: string; init: { method?: string } }) => { status: number; body: unknown }
  ) {
    return recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      const { status, body } = handler(call);
      return fakeResponse({ status, body: JSON.stringify(body) });
    });
  }

  it("parses the rich details rows", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([details]) });
    const parsed = await listProjectDetails(client, "acme");
    expect(parsed[0]?.identifier).toBe("CT");
    expect(parsed[0]?.member_role).toBe(20);
  });

  it("rejects details rows without an id", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([{ ...details, id: "nope" }]) });
    const error = await listProjectDetails(client, "acme").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("creates a project and posts the input as JSON", async () => {
    const transport = stubbedApi(() => ({ status: 201, body: details }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const created = await createProject(client, "acme", { name: "Contract", identifier: "CT" });
    expect(created.id).toBe(UUID2);
    const createCall = transport.calls.find((call) => call.url.endsWith("/projects/"));
    expect(createCall?.init.body).toBe(JSON.stringify({ name: "Contract", identifier: "CT" }));
  });

  it("deletes, unarchives and leaves by status", async () => {
    const transport = stubbedApi(() => ({ status: 204, body: null }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(deleteProject(client, "acme", UUID2)).resolves.toBe(204);
    await expect(unarchiveProject(client, "acme", UUID2)).resolves.toBe(204);
    await expect(leaveProject(client, "acme", UUID2)).resolves.toBe(204);
    expect(transport.calls.some((call) => call.url.endsWith("/members/leave/"))).toBe(true);
  });

  it("parses the archive stamp", async () => {
    const transport = stubbedApi(() => ({ status: 200, body: { archived_at: "2026-10-12 00:00:00+00:00" } }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await archiveProject(client, "acme", UUID2);
    expect(parsed.archived_at).toContain("2026-10-12");
  });

  it("lists invitations and joins by project ids", async () => {
    const invitation = {
      id: UUID,
      project: {
        id: UUID2,
        identifier: "CT",
        name: "Contract",
        cover_image: null,
        cover_image_url: null,
        logo_props: {},
        description: "",
        is_default: false,
      },
      workspace: { id: UUID, name: "Acme", slug: "acme", logo_url: null },
      email: "ada@example.com",
      accepted: false,
      token: "tok",
      message: null,
      responded_at: null,
      role: 15,
      created_by: null,
      updated_by: null,
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-01T00:00:00Z",
    };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse({
        body: JSON.stringify(call.init.method === "POST" ? { message: "Projects joined successfully" } : [invitation]),
      });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const listed = await listProjectInvitations(client, "acme");
    expect(listed[0]?.project.identifier).toBe("CT");
    await expect(joinProjects(client, "acme", [UUID2])).resolves.toEqual({
      message: "Projects joined successfully",
    });
    const joinCall = transport.calls.find((call) => call.init.method === "POST");
    expect(joinCall?.init.body).toBe(JSON.stringify({ project_ids: [UUID2] }));
  });

  it("runs favorite add, list and remove", async () => {
    const favorite = {
      id: UUID,
      entity_type: "project",
      entity_identifier: UUID2,
      entity_data: null,
      name: "Contract",
      is_folder: false,
      sequence: 0,
      parent: null,
      workspace_id: UUID,
      project_id: UUID2,
    };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "DELETE" || call.init.method === "POST") {
        return fakeResponse({ status: 204, body: "" });
      }
      return fakeResponse({ body: JSON.stringify([favorite]) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(addFavoriteProject(client, "acme", UUID2)).resolves.toBe(204);
    const listed = await listFavoriteProjects(client, "acme");
    expect(listed[0]?.entity_type).toBe("project");
    await expect(removeFavoriteProject(client, "acme", UUID2)).resolves.toBe(204);
  });

  const workspaceProps = {
    id: UUID,
    workspace: UUID2,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: "ACCORDION",
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };

  it("reads and patches workspace user properties", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse({
        body: JSON.stringify(
          call.init.method === "PATCH" ? { ...workspaceProps, navigation_project_limit: 5 } : workspaceProps
        ),
      });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const read = await getWorkspaceUserProperties(client, "acme");
    expect(read.navigation_control_preference).toBe("ACCORDION");
    const updated = await updateWorkspaceUserProperties(client, "acme", { navigation_project_limit: 5 });
    expect(updated.navigation_project_limit).toBe(5);
  });

  it("reads and patches project user properties", async () => {
    const projectProps = {
      ...workspaceProps,
      project: UUID2,
      preferences: {},
      sort_order: 65535,
    };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse({
        body: JSON.stringify(call.init.method === "PATCH" ? { ...projectProps, sort_order: 1 } : projectProps),
      });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const read = await getProjectUserProperties(client, "acme", UUID2);
    expect(read.sort_order).toBe(65535);
    const updated = await updateProjectUserProperties(client, "acme", UUID2, { sort_order: 1 });
    expect(updated.sort_order).toBe(1);
  });
});
