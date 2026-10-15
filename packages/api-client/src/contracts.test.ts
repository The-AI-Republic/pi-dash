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
  getInstanceBootstrap,
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
import { getMe, getMeSettings, markOnboarded, updateMe } from "./contracts/users.js";
import {
  checkWorkspaceSlug,
  createWorkspace,
  getMyWorkspaceMembership,
  listWorkspaceMembers,
  listWorkspaces,
} from "./contracts/workspaces.js";
import {
  acceptWorkspaceInvitations,
  answerWorkspaceInvitation,
  createWorkspaceJoinRequest,
  getWorkspaceInvitation,
  listWorkspaceInvitations,
  listWorkspaceJoinRequests,
  sendWorkspaceInvitations,
} from "./contracts/invitations.js";
import {
  addFavoriteProject,
  archiveProject,
  checkProjectIdentifier,
  createProject,
  deleteProject,
  getMyProjectRoles,
  getProject,
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
  updateProject,
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
import { uploadUserAvatar } from "./contracts/assets.js";
import { listCycles, updateCycle } from "./contracts/cycles.js";
import { listModules, updateModule } from "./contracts/modules.js";
import {
  addPageFavorite,
  archivePage,
  lockPage,
  removePageFavorite,
  restorePage,
  setPageAccess,
  unlockPage,
  updatePage,
} from "./contracts/pages.js";
import { listProjectEstimates, selectActiveEstimate } from "./contracts/estimates.js";
import { addIssueToCycle, deleteIssue, removeIssueFromCycle, setIssueModules } from "./contracts/issues.js";
import {
  checkInstanceWorkspaceSlug,
  createInstanceWorkspace,
  listInstanceWorkspaces,
} from "./contracts/admin-workspaces.js";
import {
  createLoopJob,
  deleteLoopJob,
  getLoopJob,
  listLoopJobs,
  listLoopTargets,
  updateLoopJob,
} from "./contracts/admin-loop.js";
import {
  adminSignIn,
  adminSignOut,
  adminSignUp,
  checkEmailCredentials,
  cloudSignInUrl,
  disableEmailFeature,
  getAdminSession,
  listInstanceAdmins,
  listInstanceConfigurations,
  probeCloudAuthSession,
  updateInstance,
  updateInstanceConfigurations,
} from "./contracts/instance.js";
import { fakeResponse, recordingTransport } from "./client.test.js";
import type { RecordedCall } from "./client.test.js";

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

  it("patches the name and avatar and parses the update echo", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            body: JSON.stringify({
              id: UUID,
              email: "ada@example.com",
              username: "ada",
              first_name: "Ada",
              last_name: "Byron",
              display_name: "Ada Byron",
              avatar: "https://cdn.test/ada.png",
              user_timezone: "UTC",
            }),
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const updated = await updateMe(client, { last_name: "Byron", avatar: "https://cdn.test/ada.png" });
    expect(updated.display_name).toBe("Ada Byron");
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe("https://api.test/api/users/me/");
    expect(patchCall?.init.body).toBe(JSON.stringify({ last_name: "Byron", avatar: "https://cdn.test/ada.png" }));
  });

  it("stamps the onboard marker", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify({ message: "Updated successfully" }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await markOnboarded(client, { is_onboarded: true });
    expect(result).toEqual({ message: "Updated successfully" });
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe("https://api.test/api/users/me/onboard/");
    expect(patchCall?.init.body).toBe(JSON.stringify({ is_onboarded: true }));
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

  it("checks slug availability through the query param", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify({ status: true }) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkWorkspaceSlug(client, "fresh-slug");
    expect(parsed).toEqual({ status: true });
    expect(transport.calls[0]?.url).toBe("https://api.test/api/workspace-slug-check/?slug=fresh-slug");
  });

  it("creates a workspace and parses the created row", async () => {
    const row = {
      id: UUID,
      name: "Acme",
      slug: "acme",
      logo: null,
      logo_asset: null,
      logo_url: null,
      owner: UUID2,
      organization_size: "2-10",
      timezone: "UTC",
      background_color: "#fff",
      total_members: 1,
      role: 20,
      created_by: UUID2,
      updated_by: null,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-01-02T03:04:05Z",
    };
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 201, body: JSON.stringify(row) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const created = await createWorkspace(client, { name: "Acme", slug: "acme", organization_size: "2-10" });
    expect(created.slug).toBe("acme");
    expect(created.role).toBe(20);
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe("https://api.test/api/workspaces/");
    expect(postCall?.init.body).toBe(JSON.stringify({ name: "Acme", slug: "acme", organization_size: "2-10" }));
  });

  it("parses my workspace membership", async () => {
    const member = {
      id: UUID,
      workspace: UUID2,
      member: UUID,
      role: 20,
      company_role: null,
      view_props: {},
      default_props: {},
      issue_props: {},
      is_active: true,
      getting_started_checklist: {},
      tips: {},
      explored_features: {},
      draft_issue_count: 2,
      created_by: null,
      updated_by: null,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json(member) });
    const parsed = await getMyWorkspaceMembership(client, "acme");
    expect(parsed?.role).toBe(20);
    expect(parsed?.draft_issue_count).toBe(2);
  });

  it("reports the null-pattern membership row as no membership", async () => {
    const empty = {
      role: null,
      company_role: "",
      view_props: null,
      default_props: null,
      issue_props: null,
      is_active: false,
      getting_started_checklist: null,
      tips: null,
      explored_features: null,
      created_by: null,
      updated_by: null,
      workspace: null,
      member: null,
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json(empty) });
    await expect(getMyWorkspaceMembership(client, "nope")).resolves.toBeNull();
  });

  it("reports null and primitive membership bodies as no membership", async () => {
    const nullish = createClient({ baseUrl: "https://api.test", transport: json(null) });
    await expect(getMyWorkspaceMembership(nullish, "acme")).resolves.toBeNull();
    const primitive = createClient({ baseUrl: "https://api.test", transport: json("oops") });
    await expect(getMyWorkspaceMembership(primitive, "acme")).resolves.toBeNull();
  });

  it("lists workspace memberships with the nested member projection", async () => {
    const member = {
      id: UUID,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: "",
      display_name: "Ada Lovelace",
      is_bot: false,
    };
    const memberships = [
      { id: UUID2, member, role: 20 },
      { id: UUID, member: { ...member, email: "ada@example.com", avatar_url: null }, role: 10 },
    ];
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(memberships) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await listWorkspaceMembers(client, "acme");
    expect(transport.calls[0]?.url).toBe("https://api.test/api/workspaces/acme/members/");
    expect(parsed).toHaveLength(2);
    expect(parsed[0]?.member.display_name).toBe("Ada Lovelace");
    expect(parsed[1]?.role).toBe(10);
    expect(parsed[1]?.member.avatar_url).toBeNull();
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

  it("parses my project roles keyed by project id", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify({ [UUID2]: 20 }) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await getMyProjectRoles(client, "acme");
    expect(parsed).toEqual({ [UUID2]: 20 });
    expect(transport.calls[0]?.url).toBe("https://api.test/api/users/me/workspaces/acme/project-roles/");
  });

  it("parses one project row without the list-only intake count", async () => {
    const { intake_count: _listOnly, ...detail } = project;
    const client = createClient({ baseUrl: "https://api.test", transport: json(detail) });
    const parsed = await getProject(client, "acme", UUID);
    expect(parsed.identifier).toBe("WEB");
    expect(parsed.member_role).toBe(20);
    expect("intake_count" in parsed).toBe(false);
  });

  it("hits the project detail URL", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: "{}" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(getProject(client, "acme", UUID)).rejects.toThrow();
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID}/`);
  });

  it("surfaces denial statuses with the server detail", async () => {
    const transport = recordingTransport(() => fakeResponse({ status: 404, body: '{"error":"nope"}' }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const failure = await getProject(client, "acme", UUID).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
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
    expect(parsed.is_epic).toBeUndefined();
  });

  it("parses the epic marker when the detail carries it", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({ ...issueDetail, is_epic: true }) });
    const parsed = await getIssue(client, "acme", UUID2, UUID);
    expect(parsed.is_epic).toBe(true);
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

  it("patches the onboarding fields and the last-workspace stamp", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({
            body: JSON.stringify({
              ...profile,
              role: "designer",
              use_case: "planning",
              has_marketing_email_consent: true,
              onboarding_step: { profile_complete: true },
              last_workspace_id: UUID2,
            }),
          })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const patched = await updateProfile(client, {
      role: "designer",
      use_case: "planning",
      has_marketing_email_consent: true,
      onboarding_step: { profile_complete: true },
      last_workspace_id: UUID2,
    });
    expect(patched.role).toBe("designer");
    expect(patched.use_case).toBe("planning");
    expect(patched.has_marketing_email_consent).toBe(true);
    expect(patched.last_workspace_id).toBe(UUID2);
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.init.body).toBe(
      JSON.stringify({
        role: "designer",
        use_case: "planning",
        has_marketing_email_consent: true,
        onboarding_step: { profile_complete: true },
        last_workspace_id: UUID2,
      })
    );
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

describe("thin contextual palette contracts", () => {
  function shellClient(handler: (call: RecordedCall) => { body: string }) {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse(handler(call));
    });
    return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
  }

  it("deletes an issue and posts the membership writes", async () => {
    const { client, transport } = shellClient((call) => ({
      body: JSON.stringify(call.init.method === "POST" ? { message: "success" } : {}),
    }));
    await deleteIssue(client, "acme", UUID2, UUID);
    const added = await addIssueToCycle(client, "acme", UUID2, UUID, UUID2);
    expect(added).toEqual({ message: "success" });
    await removeIssueFromCycle(client, "acme", UUID2, UUID, UUID2);
    const modules = await setIssueModules(client, "acme", UUID2, UUID2, { modules: [UUID], removed_modules: [] });
    expect(modules).toEqual({ message: "success" });
    const urls = transport.calls.map((call) => `${call.init.method} ${call.url}`);
    expect(urls.some((line) => line.startsWith("DELETE") && line.endsWith(`/issues/${UUID}/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("POST") && line.includes("/cycle-issues/"))).toBe(true);
    expect(urls.some((line) => line.startsWith("DELETE") && line.includes("/cycle-issues/"))).toBe(true);
    expect(urls.some((line) => line.startsWith("POST") && line.endsWith(`/issues/${UUID2}/modules/`))).toBe(true);
  });

  it("lists cycles, modules and estimates for the pickers", async () => {
    const cycles = [{ id: UUID, project_id: UUID2, name: "Sprint 1", is_favorite: false, total_issues: 3 }];
    const modules = [
      { id: UUID, project_id: UUID2, name: "Auth", status: "backlog", is_favorite: true, total_issues: 1 },
    ];
    const estimates = [
      {
        id: UUID,
        name: "Fibonacci",
        type: "points",
        last_used: true,
        points: [
          { id: UUID2, key: 1, value: "1" },
          { id: UUID, key: 2, value: "2" },
        ],
      },
    ];
    const { client } = shellClient((call) => ({
      body: JSON.stringify(
        call.url.includes("/cycles/") ? cycles : call.url.includes("/modules/") ? modules : estimates
      ),
    }));
    const listedCycles = await listCycles(client, "acme", UUID2);
    expect(listedCycles).toEqual([{ id: UUID, project_id: UUID2, name: "Sprint 1", is_favorite: false }]);
    const listedModules = await listModules(client, "acme", UUID2);
    expect(listedModules).toEqual([
      { id: UUID, project_id: UUID2, name: "Auth", status: "backlog", is_favorite: true },
    ]);
    const listedEstimates = await listProjectEstimates(client, "acme", UUID2);
    expect(listedEstimates).toHaveLength(1);
    expect(selectActiveEstimate(listedEstimates)?.points).toHaveLength(2);
    expect(selectActiveEstimate([])).toBeUndefined();
    expect(selectActiveEstimate([{ ...listedEstimates[0]!, last_used: false }])).toBeUndefined();
  });

  it("drives the page lock, access, archive and favorite endpoints", async () => {
    const { client, transport } = shellClient((call) => ({
      body: JSON.stringify(call.url.endsWith("/archive/") ? { archived_at: "2026-10-15T00:00:00Z" } : {}),
    }));
    await lockPage(client, "acme", UUID2, UUID);
    await unlockPage(client, "acme", UUID2, UUID);
    await setPageAccess(client, "acme", UUID2, UUID, 1);
    const archived = await archivePage(client, "acme", UUID2, UUID);
    expect(archived).toEqual({ archived_at: "2026-10-15T00:00:00Z" });
    await restorePage(client, "acme", UUID2, UUID);
    await addPageFavorite(client, "acme", UUID2, UUID);
    await removePageFavorite(client, "acme", UUID2, UUID);
    const urls = transport.calls.map((call) => `${call.init.method} ${call.url}`);
    expect(urls.some((line) => line.startsWith("POST") && line.endsWith(`/pages/${UUID}/lock/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("DELETE") && line.endsWith(`/pages/${UUID}/lock/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("POST") && line.endsWith(`/pages/${UUID}/access/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("POST") && line.endsWith(`/pages/${UUID}/archive/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("DELETE") && line.endsWith(`/pages/${UUID}/archive/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("POST") && line.includes(`/favorite-pages/${UUID}/`))).toBe(true);
    expect(urls.some((line) => line.startsWith("DELETE") && line.includes(`/favorite-pages/${UUID}/`))).toBe(true);
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

  it("patches a project and parses the updated row", async () => {
    const transport = stubbedApi(() => ({ status: 200, body: { ...details, description: "Updated" } }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const updated = await updateProject(client, "acme", UUID2, { description: "Updated" });
    expect(updated.id).toBe(UUID2);
    expect(updated.description).toBe("Updated");
    const patchCall = transport.calls.find((call) => call.url.endsWith(`/projects/${UUID2}/`));
    expect(patchCall?.init.method).toBe("PATCH");
    expect(patchCall?.init.body).toBe(JSON.stringify({ description: "Updated" }));
  });

  it("checks identifier availability and parses matches", async () => {
    const check = { exists: 1, identifiers: [{ id: 7, name: "CT", project: UUID2 }] };
    const transport = stubbedApi(() => ({ status: 200, body: check }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkProjectIdentifier(client, "acme", "ct");
    expect(parsed.exists).toBe(1);
    expect(parsed.identifiers[0]?.project).toBe(UUID2);
    const checkCall = transport.calls.find((call) => call.url.includes("/project-identifiers/"));
    expect(checkCall?.url).toContain("name=ct");
  });

  it("reports a free identifier as zero matches", async () => {
    const transport = stubbedApi(() => ({ status: 200, body: { exists: 0, identifiers: [] } }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkProjectIdentifier(client, "acme", "ZZZFREE");
    expect(parsed.exists).toBe(0);
    expect(parsed.identifiers).toEqual([]);
  });

  it("posts the full create payload the dialog assembles", async () => {
    const transport = stubbedApi(() => ({ status: 201, body: details }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const input = {
      name: "Contract",
      identifier: "CT",
      description: "Team space",
      network: 2,
      project_lead: null,
      logo_props: { in_use: "emoji", emoji: { value: "🌊" } },
      timezone: "Asia/Singapore",
      cycle_view: false,
      module_view: false,
      issue_views_view: false,
      page_view: false,
      intake_view: false,
    };
    await createProject(client, "acme", input);
    const createCall = transport.calls.find((call) => call.url.endsWith("/projects/"));
    expect(createCall?.init.body).toBe(JSON.stringify(input));
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

  it("patches project navigation preferences", async () => {
    const projectProps = {
      ...workspaceProps,
      project: UUID2,
      preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
      sort_order: 65535,
    };
    const navigation = { default_tab: "cycles", hide_in_more_menu: ["pages"] };
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      return fakeResponse({
        body: JSON.stringify(
          call.init.method === "PATCH" ? { ...projectProps, preferences: { navigation } } : projectProps
        ),
      });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const updated = await updateProjectUserProperties(client, "acme", UUID2, {
      preferences: { navigation },
    });
    expect(updated.preferences).toEqual({ navigation });
    const patch = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patch?.init.body).toBe(JSON.stringify({ preferences: { navigation } }));
  });
});

describe("invitation contracts", () => {
  const invitation = {
    id: UUID,
    workspace: { id: UUID2, name: "Acme", slug: "acme", logo_url: null },
    email: "ada@example.com",
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: "/workspace-invitations/?invitation_id=1&slug=acme&token=tok",
    created_by: UUID2,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };

  it("lists my workspace invitations", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify([invitation]) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await listWorkspaceInvitations(client);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.workspace.slug).toBe("acme");
    expect(transport.calls[0]?.url).toBe("https://api.test/api/users/me/workspaces/invitations/");
  });

  it("batch-accepts invitations and answers nothing (204)", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 204, body: "" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(acceptWorkspaceInvitations(client, [UUID])).resolves.toBeUndefined();
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe("https://api.test/api/users/me/workspaces/invitations/");
    expect(postCall?.init.body).toBe(JSON.stringify({ invitations: [UUID] }));
  });

  it("fetches one invitation and answers it", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "POST") {
        return fakeResponse({ body: JSON.stringify({ message: "Workspace Invitation Accepted" }) });
      }
      return fakeResponse({ body: JSON.stringify(invitation) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const fetched = await getWorkspaceInvitation(client, "acme", UUID);
    expect(fetched.email).toBe("ada@example.com");
    const answered = await answerWorkspaceInvitation(client, "acme", UUID, { accepted: true, token: "tok" });
    expect(answered).toEqual({ message: "Workspace Invitation Accepted" });
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe(`https://api.test/api/workspaces/acme/invitations/${UUID}/join/`);
    expect(postCall?.init.body).toBe(JSON.stringify({ accepted: true, token: "tok" }));
  });

  it("sends bulk workspace invitations", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify({ message: "Emails sent successfully" }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await sendWorkspaceInvitations(client, "acme", {
      emails: [{ email: "ada@example.com", role: 15 }],
    });
    expect(result).toEqual({ message: "Emails sent successfully" });
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe("https://api.test/api/workspaces/acme/invitations/");
    expect(postCall?.init.body).toBe(JSON.stringify({ emails: [{ email: "ada@example.com", role: 15 }] }));
  });

  it("lists my workspace join requests", async () => {
    const request = {
      id: UUID,
      requester: {
        id: UUID2,
        first_name: "Ada",
        last_name: "Lovelace",
        avatar: "",
        avatar_url: null,
        display_name: "ada",
        is_bot: false,
      },
      admin_email: "root@example.com",
      message: null,
      status: "PENDING",
      responded_at: null,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-01-02T03:04:05Z",
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json([request]) });
    const parsed = await listWorkspaceJoinRequests(client);
    expect(parsed[0]?.status).toBe("PENDING");
    expect(parsed[0]?.requester.avatar_url).toBeNull();
  });

  it("creates a join request and parses both outcomes", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      const body = JSON.parse(call.init.body as string) as { admin_email: string };
      return body.admin_email === "member@example.com"
        ? fakeResponse({
            body: JSON.stringify({ message: "You are already a member of this workspace", workspace_slug: "acme" }),
          })
        : fakeResponse({ status: 201, body: JSON.stringify({ message: "Request sent" }) });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const sent = await createWorkspaceJoinRequest(client, { admin_email: "root@example.com" });
    expect(sent).toEqual({ message: "Request sent" });
    const shortcut = await createWorkspaceJoinRequest(client, { admin_email: "member@example.com" });
    expect(shortcut).toEqual({ message: "You are already a member of this workspace", workspace_slug: "acme" });
    const postCalls = transport.calls.filter((call) => call.init.method === "POST");
    expect(postCalls).toHaveLength(2);
    expect(postCalls[0]?.url).toBe("https://api.test/api/users/me/workspaces/join-requests/");
  });
});

describe("assets contracts", () => {
  const grant = {
    asset_id: "asset-1",
    asset_url: "https://cdn.test/avatars/asset-1.png",
    upload_data: { url: "https://uploads.test/signed", fields: { key: "avatars/asset-1.png", policy: "p" } },
  };

  function grantTransport() {
    return recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) {
        return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
      }
      if (call.init.method === "POST") {
        return fakeResponse({ status: 201, body: JSON.stringify(grant) });
      }
      return fakeResponse({ body: JSON.stringify({ ok: true }) });
    });
  }

  it("uploads an avatar through grant, bytes, and confirm", async () => {
    const transport = grantTransport();
    const client = createClient({ baseUrl: "https://api.test", transport });
    const file = new File(["bytes"], "ada.png", { type: "image/png" });
    const uploads: Array<{ url: string; body: FormData }> = [];
    const url = await uploadUserAvatar(client, file, async (target, body) => {
      uploads.push({ url: target, body });
      return { ok: true, status: 204 };
    });
    expect(url).toBe("https://cdn.test/avatars/asset-1.png");

    const grantCall = transport.calls.find((call) => call.init.method === "POST");
    expect(grantCall?.url).toBe("https://api.test/api/assets/v2/user-assets/");
    expect(grantCall?.init.body).toBe(
      JSON.stringify({
        entity_identifier: "",
        entity_type: "USER_AVATAR",
        name: "ada.png",
        size: file.size,
        type: "image/png",
      })
    );

    expect(uploads).toHaveLength(1);
    expect(uploads[0]?.url).toBe("https://uploads.test/signed");
    const payload = uploads[0]?.body;
    expect(payload?.get("key")).toBe("avatars/asset-1.png");
    expect(payload?.get("policy")).toBe("p");
    expect(payload?.get("file")).toBe(file);

    const confirmCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(confirmCall?.url).toBe("https://api.test/api/assets/v2/user-assets/asset-1/");
  });

  it("rejects when the signed upload refuses the bytes", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: grantTransport() });
    const file = new File(["bytes"], "ada.png", { type: "image/png" });
    const failure = await uploadUserAvatar(client, file, async () => ({ ok: false, status: 403 })).then(
      () => null,
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(403);
  });

  it("POSTs the bytes through the global fetch by default", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: grantTransport() });
    const file = new File(["bytes"], "ada.png", { type: "image/png" });
    const seen: Array<{ url: string; init: { method?: string; body?: unknown } }> = [];
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async (url: string, init: { method?: string; body?: unknown }) => {
      seen.push({ url, init });
      return { ok: true, status: 204 };
    }) as typeof fetch;
    try {
      await uploadUserAvatar(client, file);
    } finally {
      globalThis.fetch = realFetch;
    }
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://uploads.test/signed");
    expect(seen[0]?.init.method).toBe("POST");
    expect(seen[0]?.init.body).toBeInstanceOf(FormData);
  });
});

describe("instance-admin workspace contracts", () => {
  const row = {
    id: UUID,
    name: "Acme",
    slug: "acme",
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: { id: UUID2, email: "root@example.com", first_name: "Root", last_name: "Admin" },
    organization_size: "2-10",
    timezone: "UTC",
    background_color: "#fff",
    total_projects: 2,
    total_members: 3,
    created_by: UUID2,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };

  it("parses the cursor-paginated workspace list", async () => {
    const page = {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: 1,
      next_cursor: "10:1:0",
      prev_cursor: "10:0:0",
      next_page_results: false,
      prev_page_results: false,
      count: 1,
      total_results: 1,
      results: [row],
    };
    const client = createClient({ baseUrl: "https://api.test", transport: json(page) });
    const parsed = await listInstanceWorkspaces(client);
    expect(parsed.total_count).toBe(1);
    expect(parsed.results[0]?.owner?.email).toBe("root@example.com");
  });

  it("sends the cursor only when set", async () => {
    const page = {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: 0,
      next_cursor: "10:1:0",
      prev_cursor: "10:0:0",
      next_page_results: false,
      prev_page_results: true,
      count: 0,
      total_results: 0,
      results: [],
    };
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(page) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await listInstanceWorkspaces(client);
    expect(transport.calls[0]?.url).toBe("https://api.test/api/instances/workspaces/");
    await listInstanceWorkspaces(client, "10:1:0");
    expect(transport.calls[1]?.url).toBe("https://api.test/api/instances/workspaces/?cursor=10%3A1%3A0");
    await listInstanceWorkspaces(client, "");
    expect(transport.calls[2]?.url).toBe("https://api.test/api/instances/workspaces/");
  });

  it("checks slug availability through the query param", async () => {
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify({ status: false }) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkInstanceWorkspaceSlug(client, "acme");
    expect(parsed).toEqual({ status: false });
    expect(transport.calls[0]?.url).toBe("https://api.test/api/instances/workspace-slug-check/?slug=acme");
  });

  it("creates a workspace without an owner field and parses the row", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 201, body: JSON.stringify(row) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const created = await createInstanceWorkspace(client, {
      name: "Acme",
      slug: "acme",
      organization_size: "2-10",
    });
    expect(created.slug).toBe("acme");
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe("https://api.test/api/instances/workspaces/");
    expect(postCall?.init.body).toBe(JSON.stringify({ name: "Acme", slug: "acme", organization_size: "2-10" }));
  });
});

describe("instance-admin loop contracts", () => {
  const job = {
    id: UUID,
    slug: "daily-digest",
    name: "Daily digest",
    public_name: "Daily digest",
    public_description: "A summary.",
    prompt: "Summarize.",
    min_role: 15,
    enabled: true,
    is_builtin: false,
    dtstart: "2024-01-02T03:04:05+00:00",
    rrule: "FREQ=DAILY;BYHOUR=2",
    tzid: "UTC",
    created_at: "2024-01-02T03:04:05+00:00",
    updated_at: "2024-01-02T03:04:05+00:00",
  };

  it("parses the job list", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([job]) });
    const parsed = await listLoopJobs(client);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.prompt).toBe("Summarize.");
  });

  it("creates a job and parses the created row", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 201, body: JSON.stringify(job) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const created = await createLoopJob(client, {
      slug: "daily-digest",
      name: "Daily digest",
      public_name: "Daily digest",
      prompt: "Summarize.",
      rrule: "FREQ=DAILY;BYHOUR=2",
    });
    expect(created.slug).toBe("daily-digest");
    const postCall = transport.calls.find((call) => call.init.method === "POST");
    expect(postCall?.url).toBe("https://api.test/api/instances/loop/jobs/");
  });

  it("parses the detail read with its stats rollup", async () => {
    const detail = { ...job, stats: { target_count: 1, completed: 0, failed: 0, skipped: 1 } };
    const client = createClient({ baseUrl: "https://api.test", transport: json(detail) });
    const parsed = await getLoopJob(client, UUID);
    expect(parsed.stats.skipped).toBe(1);
  });

  it("patches a job and parses the updated row", async () => {
    const updated = { ...job, enabled: false };
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ body: JSON.stringify(updated) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await updateLoopJob(client, UUID, { enabled: false });
    expect(parsed.enabled).toBe(false);
    const patchCall = transport.calls.find((call) => call.init.method === "PATCH");
    expect(patchCall?.url).toBe(`https://api.test/api/instances/loop/jobs/${UUID}/`);
    expect(patchCall?.init.body).toBe(JSON.stringify({ enabled: false }));
  });

  it("deletes a job and reports the status", async () => {
    const transport = recordingTransport((call) =>
      call.url.endsWith("/auth/get-csrf-token/")
        ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
        : fakeResponse({ status: 204, body: "" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const status = await deleteLoopJob(client, UUID);
    expect(status).toBe(204);
    const deleteCall = transport.calls.find((call) => call.init.method === "DELETE");
    expect(deleteCall?.url).toBe(`https://api.test/api/instances/loop/jobs/${UUID}/`);
  });

  it("parses the targets page and sends skip_reason only when set", async () => {
    const page = {
      page: 1,
      results: [
        {
          id: UUID2,
          workspace_slug: "acme",
          user_email: "ada@example.com",
          next_run_at: null,
          last_skipped_at: "2024-01-02T03:04:05+00:00",
          last_skip_reason: "user_disabled",
          last_run: null,
        },
        {
          id: UUID,
          workspace_slug: null,
          user_email: null,
          next_run_at: "2024-01-03T03:04:05+00:00",
          last_skipped_at: null,
          last_skip_reason: "",
          last_run: {
            status: "completed",
            error_code: "",
            model_used: "gpt-5",
            total_tokens: 42,
            completed_at: "2024-01-02T03:04:05+00:00",
          },
        },
      ],
    };
    const transport = recordingTransport(() => fakeResponse({ body: JSON.stringify(page) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await listLoopTargets(client, UUID);
    expect(parsed.results[0]?.last_skip_reason).toBe("user_disabled");
    expect(parsed.results[1]?.last_run?.total_tokens).toBe(42);
    expect(transport.calls[0]?.url).toBe(`https://api.test/api/instances/loop/jobs/${UUID}/targets/`);
    await listLoopTargets(client, UUID, { skipReason: "user_disabled" });
    expect(transport.calls[1]?.url).toBe(
      `https://api.test/api/instances/loop/jobs/${UUID}/targets/?skip_reason=user_disabled`
    );
    await listLoopTargets(client, UUID, { skipReason: "" });
    expect(transport.calls[2]?.url).toBe(`https://api.test/api/instances/loop/jobs/${UUID}/targets/`);
    await listLoopTargets(client, UUID, { page: 2 });
    expect(transport.calls[3]?.url).toBe(`https://api.test/api/instances/loop/jobs/${UUID}/targets/?page=2`);
  });
});

const BOOTSTRAP_CONFIG = {
  enable_signup: true,
  is_workspace_creation_disabled: false,
  is_google_enabled: false,
  is_github_enabled: false,
  is_gitlab_enabled: false,
  is_gitea_enabled: false,
  is_magic_login_enabled: true,
  is_email_password_enabled: true,
  is_smtp_configured: true,
  is_self_managed: true,
};

const INSTANCE_ROW = {
  id: UUID,
  created_at: "2025-01-02T03:04:05.000Z",
  updated_at: "2025-02-03T04:05:06.000Z",
  created_by: null,
  updated_by: UUID2,
  deleted_at: null,
  instance_name: "Acme",
  whitelist_emails: null,
  instance_id: "instance-acme",
  current_version: "1.2.3",
  latest_version: null,
  edition: "PI_DASH_COMMUNITY",
  domain: "",
  last_checked_at: "2025-03-04T05:06:07.000Z",
  namespace: null,
  is_telemetry_enabled: true,
  is_support_required: true,
  is_setup_done: true,
  is_signup_screen_visited: false,
  is_verified: false,
  is_test: false,
  is_current_version_deprecated: false,
};

const ADMIN_ME = {
  id: UUID,
  avatar: "",
  avatar_url: null,
  cover_image: null,
  date_joined: "2025-01-02T03:04:05.000Z",
  display_name: "Ada Admin",
  email: "ada@example.com",
  first_name: "Ada",
  last_name: "Admin",
  is_active: true,
  is_bot: false,
  is_email_verified: false,
  user_timezone: "UTC",
  username: "ada.admin",
  is_password_autoset: false,
};

const ADMIN_ENTRY = {
  id: UUID,
  created_at: "2025-01-02T03:04:05.000Z",
  updated_at: "2025-01-02T03:04:05.000Z",
  created_by: null,
  updated_by: null,
  deleted_at: null,
  user: UUID2,
  instance: UUID,
  role: 20,
  is_verified: false,
  user_detail: {
    id: UUID2,
    first_name: "Ada",
    last_name: "Admin",
    avatar: "",
    avatar_url: null,
    is_bot: false,
    display_name: "Ada Admin",
    email: "ada@example.com",
    last_login_medium: "email",
  },
};

const CONFIG_ROW = {
  id: UUID,
  created_at: "2025-01-02T03:04:05.000Z",
  updated_at: "2025-01-02T03:04:05.000Z",
  created_by: null,
  updated_by: null,
  deleted_at: null,
  key: "EMAIL_HOST",
  value: "mail.test",
  category: "SMTP",
  is_encrypted: false,
  source: "db",
  is_managed: false,
};

const CLOUD_USER = {
  id: UUID,
  email: "ada@example.com",
  display_name: "Ada Admin",
  first_name: "Ada",
  last_name: "Admin",
  avatar: "",
};

function csrfThen(handler: (url: string) => { status: number; body: string; url?: string }) {
  return recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse(handler(call.url))
  );
}

describe("instance bootstrap contracts", () => {
  it("reads the full bootstrap envelope", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({
        config: BOOTSTRAP_CONFIG,
        instance: { ...INSTANCE_ROW, workspaces_exist: true },
      }),
    });
    const parsed = await getInstanceBootstrap(client);
    expect(parsed.config.enable_signup).toBe(true);
    expect(parsed.instance.instance_name).toBe("Acme");
    expect(parsed.instance.workspaces_exist).toBe(true);
  });

  it("rejects a bootstrap without the instance half", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json({ config: BOOTSTRAP_CONFIG }),
    });
    const error = await getInstanceBootstrap(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("patches the name and telemetry flag together", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/")).toBe(true);
      return { status: 200, body: JSON.stringify({ ...INSTANCE_ROW, instance_name: "New name" }) };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await updateInstance(client, { instance_name: "New name", is_telemetry_enabled: false });
    expect(parsed.instance_name).toBe("New name");
    const patchCall = transport.calls.find((call) => call.url.endsWith("/api/instances/"));
    expect(patchCall?.init.method).toBe("PATCH");
    expect(patchCall?.init.body).toBe(JSON.stringify({ instance_name: "New name", is_telemetry_enabled: false }));
  });

  it("rejects a patched row missing the instance id", async () => {
    const transport = csrfThen(() => ({ status: 200, body: JSON.stringify({ ...INSTANCE_ROW, id: undefined }) }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await updateInstance(client, { instance_name: "New name" }).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });
});

describe("instance admin session contracts", () => {
  it("parses a signed-in admin", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(ADMIN_ME) });
    await expect(getAdminSession(client)).resolves.toEqual({ kind: "admin", admin: ADMIN_ME });
  });

  it("reads 401 as anonymous", async () => {
    const transport = recordingTransport(() =>
      fakeResponse({ status: 401, body: JSON.stringify({ detail: "Authentication credentials were not provided." }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(getAdminSession(client)).resolves.toEqual({ kind: "anonymous" });
  });

  it("reads 403 as forbidden", async () => {
    const transport = recordingTransport(() =>
      fakeResponse({ status: 403, body: JSON.stringify({ detail: "You do not have permission." }) })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(getAdminSession(client)).resolves.toEqual({ kind: "forbidden" });
  });

  it("rethrows a transient failure instead of classifying it", async () => {
    const transport = recordingTransport(() => fakeResponse({ status: 500, body: "boom" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await getAdminSession(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(500);
  });

  it("rethrows a contract violation instead of classifying it", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({}) });
    const error = await getAdminSession(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("lets a foreign client failure pass through unclassified", async () => {
    const foreign = new Error("foreign boom");
    const stub = { getJson: async () => Promise.reject(foreign) };
    const error = await getAdminSession(stub as never).catch((e: unknown) => e);
    expect(error).toBe(foreign);
  });

  it("lists the instance admins with their nested users", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json([ADMIN_ENTRY]) });
    const parsed = await listInstanceAdmins(client);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]?.user_detail.email).toBe("ada@example.com");
  });

  it("rejects an admin row without its nested user", async () => {
    const { user_detail: _dropped, ...row } = ADMIN_ENTRY;
    const client = createClient({ baseUrl: "https://api.test", transport: json([row]) });
    const error = await listInstanceAdmins(client).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });
});

describe("instance admin auth contracts", () => {
  it("posts the setup form with its defaults", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/admins/sign-up/")).toBe(true);
      return { status: 200, body: "<html>", url: "https://web.test/god-mode/general/" };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await adminSignUp(client, {
      firstName: "Ada",
      email: "ada@example.com",
      password: "StrongPass123!xyz",
    });
    expect(result).toEqual({ ok: true, location: "https://web.test/god-mode/general/" });
    const postCall = transport.calls.find((call) => call.url.endsWith("/api/instances/admins/sign-up/"));
    expect(postCall?.init.headers["Content-Type"]).toBe("application/x-www-form-urlencoded");
    expect(postCall?.init.headers["X-CSRFTOKEN"]).toBe("csrf");
    const body = new URLSearchParams(postCall?.init.body ?? "");
    expect(body.get("first_name")).toBe("Ada");
    expect(body.get("last_name")).toBe("");
    expect(body.get("company_name")).toBe("");
    expect(body.get("confirm_password")).toBe("StrongPass123!xyz");
    expect(body.get("is_telemetry_enabled")).toBe("True");
  });

  it("posts an opted-out setup with explicit fields", async () => {
    const transport = csrfThen(() => ({ status: 200, body: "<html>", url: "https://web.test/god-mode/general/" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await adminSignUp(client, {
      firstName: "Ada",
      lastName: "Admin",
      email: "ada@example.com",
      companyName: "Acme",
      password: "StrongPass123!xyz",
      confirmPassword: "StrongPass123!xyz",
      telemetryEnabled: false,
    });
    const postCall = transport.calls.find((call) => call.url.endsWith("/api/instances/admins/sign-up/"));
    const body = new URLSearchParams(postCall?.init.body ?? "");
    expect(body.get("last_name")).toBe("Admin");
    expect(body.get("company_name")).toBe("Acme");
    expect(body.get("is_telemetry_enabled")).toBe("False");
  });

  it("reads a rejected setup from its error code", async () => {
    const location = "https://web.test/god-mode/?error_code=5150&error_message=ADMIN_ALREADY_EXIST";
    const transport = csrfThen(() => ({ status: 200, body: "<html>", url: location }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await adminSignUp(client, {
      firstName: "Ada",
      email: "ada@example.com",
      password: "StrongPass123!xyz",
    });
    expect(result).toEqual({ ok: false, location, code: "5150", message: "ADMIN_ALREADY_EXIST" });
  });

  it("posts the sign-in form and reads success from the landing URL", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/admins/sign-in/")).toBe(true);
      return { status: 200, body: "<html>", url: "https://web.test/god-mode/general/" };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await adminSignIn(client, { email: "ada@example.com", password: "secret" });
    expect(result).toEqual({ ok: true, location: "https://web.test/god-mode/general/" });
    const postCall = transport.calls.find((call) => call.url.endsWith("/api/instances/admins/sign-in/"));
    expect(postCall?.init.headers["X-CSRFTOKEN"]).toBe("csrf");
    const body = new URLSearchParams(postCall?.init.body ?? "");
    expect(body.get("email")).toBe("ada@example.com");
    expect(body.get("password")).toBe("secret");
  });

  it("reads a failed sign-in from its error code", async () => {
    const location = "https://web.test/god-mode/?error_code=5175&error_message=ADMIN_AUTHENTICATION_FAILED";
    const transport = csrfThen(() => ({ status: 200, body: "<html>", url: location }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const result = await adminSignIn(client, { email: "ada@example.com", password: "nope" });
    expect(result).toEqual({ ok: false, location, code: "5175", message: "ADMIN_AUTHENTICATION_FAILED" });
  });

  it("posts sign-out and reports the landing URL", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/admins/sign-out/")).toBe(true);
      return { status: 200, body: "<html>", url: "https://web.test/god-mode/" };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(adminSignOut(client)).resolves.toEqual({ location: "https://web.test/god-mode/" });
  });
});

describe("instance configuration contracts", () => {
  it("lists the key map with managed flags", async () => {
    const managed = { ...CONFIG_ROW, id: UUID2, key: "LLM_API_KEY", value: null, source: "env", is_managed: true };
    const client = createClient({ baseUrl: "https://api.test", transport: json([CONFIG_ROW, managed]) });
    const parsed = await listInstanceConfigurations(client);
    expect(parsed).toHaveLength(2);
    expect(parsed[1]).toMatchObject({ key: "LLM_API_KEY", value: null, source: "env", is_managed: true });
  });

  it("rejects a row with an unknown source", async () => {
    const client = createClient({
      baseUrl: "https://api.test",
      transport: json([{ ...CONFIG_ROW, source: "ssm" }]),
    });
    const error = await listInstanceConfigurations(client).catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");
  });

  it("patches a partial string map and reads back the echo", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/configurations/")).toBe(true);
      return { status: 200, body: JSON.stringify([{ ...CONFIG_ROW, value: "smtp.test" }]) };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await updateInstanceConfigurations(client, { EMAIL_HOST: "smtp.test" });
    expect(parsed).toEqual([{ ...CONFIG_ROW, value: "smtp.test" }]);
    const patchCall = transport.calls.find((call) => call.url.endsWith("/api/instances/configurations/"));
    expect(patchCall?.init.method).toBe("PATCH");
    expect(patchCall?.init.body).toBe(JSON.stringify({ EMAIL_HOST: "smtp.test" }));
  });

  it("disables the email feature and reports the status", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/configurations/disable-email-feature/")).toBe(true);
      return { status: 200, body: "" };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(disableEmailFeature(client)).resolves.toBe(200);
    const deleteCall = transport.calls.find((call) =>
      call.url.endsWith("/api/instances/configurations/disable-email-feature/")
    );
    expect(deleteCall?.init.method).toBe("DELETE");
  });

  it("surfaces a failed disable instead of swallowing it", async () => {
    const transport = csrfThen(() => ({
      status: 400,
      body: JSON.stringify({ error: "Failed to disable email configuration" }),
    }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await disableEmailFeature(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
  });

  it("checks mail credentials with a receiver-only payload", async () => {
    const transport = csrfThen((url) => {
      expect(url.endsWith("/api/instances/email-credentials-check/")).toBe(true);
      return { status: 200, body: JSON.stringify({ message: "Email successfully sent." }) };
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const parsed = await checkEmailCredentials(client, "probe@example.com");
    expect(parsed).toEqual({ message: "Email successfully sent." });
    const postCall = transport.calls.find((call) => call.url.endsWith("/api/instances/email-credentials-check/"));
    expect(postCall?.init.body).toBe(JSON.stringify({ receiver_email: "probe@example.com" }));
  });

  it("surfaces the server mail error on a failed check", async () => {
    const transport = csrfThen(() => ({
      status: 400,
      body: JSON.stringify({ error: "Invalid credentials provided" }),
    }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await checkEmailCredentials(client, "probe@example.com").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).message).toBe("Invalid credentials provided");
  });
});

describe("cloud auth probe contracts", () => {
  it("reads a signed-in probe with its user", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json(CLOUD_USER) });
    await expect(probeCloudAuthSession(client)).resolves.toEqual({ kind: "authenticated", user: CLOUD_USER });
  });

  it("reads 401 and 403 as anonymous", async () => {
    for (const status of [401, 403]) {
      const transport = recordingTransport(() => fakeResponse({ status, body: JSON.stringify({ detail: "denied" }) }));
      const client = createClient({ baseUrl: "https://api.test", transport });
      await expect(probeCloudAuthSession(client)).resolves.toEqual({ kind: "anonymous" });
    }
  });

  it("reads other statuses as inconclusive", async () => {
    for (const status of [404, 500]) {
      const transport = recordingTransport(() => fakeResponse({ status, body: "nope" }));
      const client = createClient({ baseUrl: "https://api.test", transport });
      await expect(probeCloudAuthSession(client)).resolves.toEqual({ kind: "inconclusive" });
    }
  });

  it("reads a network failure as inconclusive", async () => {
    const transport = recordingTransport(() => {
      throw new Error("fetch failed");
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    await expect(probeCloudAuthSession(client)).resolves.toEqual({ kind: "inconclusive" });
  });

  it("rethrows a contract violation instead of guessing", async () => {
    const client = createClient({ baseUrl: "https://api.test", transport: json({}) });
    const error = await probeCloudAuthSession(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("rethrows a caller abort instead of guessing", async () => {
    const transport = recordingTransport(() => {
      throw new ApiError({ status: 0, code: "aborted", message: "Request was aborted" });
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await probeCloudAuthSession(client).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("aborted");
  });

  it("lets a foreign client failure pass through unguessed", async () => {
    const foreign = new Error("foreign boom");
    const stub = { getJson: async () => Promise.reject(foreign) };
    const error = await probeCloudAuthSession(stub as never).catch((e: unknown) => e);
    expect(error).toBe(foreign);
  });

  it("builds the sign-in entry with an encoded deep-link return", async () => {
    expect(cloudSignInUrl("https://api.test/", "/god-mode/loop/")).toBe(
      "https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2Floop%2F"
    );
    expect(cloudSignInUrl("https://api.test", "/god-mode/?tab=email&x=1")).toBe(
      "https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2F%3Ftab%3Demail%26x%3D1"
    );
  });
});
