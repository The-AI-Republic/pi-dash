// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Server-state assertions for parity scenarios (NEWFRONT-19). The driver
// proves what the user sees; this helper proves what the server stored, so
// a redesigned screen cannot pass while saving the wrong thing. It signs
// in through the same native credential endpoint the old sign-in card
// posts to, then reads back through the public REST API with that session.
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { promisify } from "node:util";
import type { ParitySeedFacts } from "../drivers/parity-driver";

const execFileAsync = promisify(execFile);

function apiBaseFromEnv(): string {
  const raw = (process.env["PARITY_API_URL"] ?? "http://localhost:18019").trim().replace(/\/+$/, "");
  return raw;
}

/**
 * Fetch that rides out HTTP 429s. The seeded stack throttles anonymous calls
 * per minute per IP (shared by every parallel run on this machine), and a
 * throttled request never reaches its view — so retrying it cannot
 * double-apply anything. Backs off between attempts, then returns the last
 * response for the caller to interpret.
 */
async function fetchTolerant(input: string, init?: RequestInit, retries = 5): Promise<Response> {
  const backoffMs = [5000, 10000, 20000, 30000, 45000];
  for (let attempt = 0; ; attempt++) {
    const res = await fetch(input, init);
    if (res.status !== 429 || attempt >= retries) return res;
    await new Promise((resolve) => setTimeout(resolve, backoffMs[Math.min(attempt, backoffMs.length - 1)]));
  }
}
// --- Auth sign-up/recovery helpers (NEWFRONT-108). Appended; existing
// --- helpers above are untouched per the shared harness contract.
// --- uniqueEmail lives with the base harness (single definition shared by
// --- all areas); the scenarios below call it with explicit prefixes.

/** Run a Django shell snippet inside the stack's api container; resolves with stdout. */
export async function apiShell(python: string): Promise<string> {
  // Slots verifying against a prefixed stack point this at their own api
  // container; the shared parity19 default is unchanged (NEWFRONT-42).
  const container = process.env["PARITY_API_CONTAINER"] ?? "parity19-api";
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", container, "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/** Mint a password-reset (uid, token) pair for an existing user without sending mail. */
export async function mintPasswordResetToken(email: string): Promise<{ uid: string; token: string }> {
  const out = await apiShell(
    `from pi_dash.db.models import User\n` +
      `from pi_dash.authentication.views.app.password_management import generate_password_token\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `uid, token = generate_password_token(user)\n` +
      `print("PARITY_UID:" + uid)\n` +
      `print("PARITY_TOKEN:" + token)\n`
  );
  const uid = /^PARITY_UID:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const token = /^PARITY_TOKEN:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (uid === "" || token === "") throw new Error(`[parity] reset-token mint produced no pair for ${email}.`);
  return { uid, token };
}

/** Plant a magic-code in Redis for an address, standing in for the emailed code. */
export async function mintMagicCode(email: string, code?: string): Promise<string> {
  const value = code ?? String(Math.floor(100000 + Math.random() * 900000));
  const payload = JSON.stringify({ current_attempt: 0, email, token: value });
  await execFileAsync(
    "docker",
    ["exec", "-i", "parity19-redis", "redis-cli", "SET", `magic_${email}`, payload, "EX", "600"],
    {
      timeout: 60_000,
    }
  );
  return value;
}

/**
 * Flip the instance mail switch for one scenario. The seeded stack is
 * mail-less by default (sibling AUTH-008 proves that); scenarios that need
 * the working mail path set it and restore it in teardown.
 */
export async function setSmtpConfigured(on: boolean): Promise<void> {
  const value = on ? "parity19-scratch-smtp" : "";
  await apiShell(
    `from pi_dash.license.models import InstanceConfiguration\n` +
      `InstanceConfiguration.objects.filter(key="EMAIL_HOST").update(value=${JSON.stringify(value)})\n` +
      `print("PARITY_SMTP_OK")\n`
  );
}

/**
 * Complete onboarding for a user through the app's own onboard endpoint
 * (the same write the client performs), then outlast the profile read's
 * browser cache (max-age 12s) so the next guard read is fresh. Flipping the
 * flag with direct SQL leaves a stale cached read behind and funnels the
 * user back to onboarding.
 */
export async function completeOnboarding(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await apiShell(
    `from pi_dash.db.models import User, Profile\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `Profile.objects.get_or_create(user=user)\n` +
      `print("PARITY_PROFILE_OK")\n`
  );
  const session = await signInSession(email, password, apiBase);
  const csrfMatch = /(?:^|;\s*)csrftoken=([^;]+)/.exec(session);
  const csrf = csrfMatch?.[1] ?? "";
  if (csrf === "") throw new Error("[parity] onboard PATCH has no CSRF token.");
  const res = await fetch(`${apiBase}/api/users/me/onboard/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: session, "X-CSRFToken": csrf },
    body: JSON.stringify({ is_onboarded: true }),
  });
  if (!res.ok) throw new Error(`[parity] onboard PATCH failed with HTTP ${res.status}.`);
  await sleep(13_000);
}

/** Server facts about a user: existence, password mode, onboarding flag. */
export async function userFacts(
  email: string
): Promise<{ exists: boolean; passwordAutoset: boolean; onboarded: boolean }> {
  const out = await apiShell(
    `import json\n` +
      `from pi_dash.db.models import User, Profile\n` +
      `user = User.objects.filter(email=${JSON.stringify(email)}).first()\n` +
      `profile = Profile.objects.filter(user=user).first() if user else None\n` +
      `print("PARITY_USER:" + json.dumps({\n` +
      `  "exists": user is not None,\n` +
      `  "passwordAutoset": bool(user and user.is_password_autoset),\n` +
      `  "onboarded": bool(profile and profile.is_onboarded),\n` +
      `}))\n`
  );
  const line = /^PARITY_USER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] user facts produced no row for ${email}.`);
  return JSON.parse(line) as { exists: boolean; passwordAutoset: boolean; onboarded: boolean };
}

/**
 * Create an account through the native sign-up POST without touching the
 * browser session. Recovery scenarios need a signed-out visitor on the
 * reset form: the app funnels signed-in unfinished users to onboarding,
 * so creating through the UI would sign the context in.
 */
export async function signUpAccount(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry(`sign-up ${email}`, async () => {
    const tokenRes = await fetch(`${apiBase}/auth/get-csrf-token/`);
    if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
    const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
    const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
    if (token === "") throw new Error("[parity] CSRF token response carried no token.");
    const preCookies = cookieHeader(setCookieHeaders(tokenRes));
    const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: token });
    const res = await fetch(`${apiBase}/auth/sign-up/`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
      body,
      redirect: "manual",
    });
    if (res.status !== 200 && res.status !== 302) {
      throw new Error(`[parity] sign-up failed with HTTP ${res.status} for ${email}.`);
    }
  });
}

/** Raw email-check answer for an address (existing vs new, credential vs code). */
export async function emailCheckStatus(
  email: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ existing: boolean; status: string }> {
  return withThrottleRetry(`email-check ${email}`, async () => {
    const res = await fetch(`${apiBase}/auth/email-check/`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email }),
    });
    if (!res.ok) throw new Error(`[parity] email-check failed with HTTP ${res.status}.`);
    return (await res.json()) as { existing: boolean; status: string };
  });
}

export function seedFactsFromEnv(): ParitySeedFacts {
  const file = process.env["PARITY_SEED_FILE"];
  if (!file)
    throw new Error("[parity] PARITY_SEED_FILE is not set; run the stack seed step first (see stack/README.md).");
  return JSON.parse(readFileSync(file, "utf8")) as ParitySeedFacts;
}

function cookieHeader(setCookies: string[]): string {
  return setCookies
    .map((line) => line.split(";", 1)[0]?.trim())
    .filter((pair) => pair !== undefined && pair.length > 0)
    .join("; ");
}

function setCookieHeaders(res: Response): string[] {
  const anyHeaders = res.headers as unknown as { getSetCookie?: () => string[] };
  if (typeof anyHeaders.getSetCookie === "function") return anyHeaders.getSetCookie();
  const single = res.headers.get("set-cookie");
  return single === null ? [] : [single];
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isThrottleFailure(error: unknown): boolean {
  return error instanceof Error && /HTTP 429/.test(error.message);
}

/**
 * The scratch stack throttles anonymous callers at 30/minute per IP, and
 * every scenario shares one bucket with the frontend's own loader calls. A
 * saturated minute answers 429; waiting out the rolling window recovers, so
 * retry throttled calls instead of failing the oracle.
 */
async function withThrottleRetry<T>(label: string, fn: () => Promise<T>, attempts = 3): Promise<T> {
  let last: unknown;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await fn();
    } catch (error) {
      last = error;
      if (!isThrottleFailure(error) || attempt === attempts) throw error;
      // eslint-disable-next-line no-console -- oracle runs surface retries in the log.
      console.log(`[parity] ${label} throttled (attempt ${attempt}/${attempts}); waiting out the window.`);
      await sleep(65_000);
    }
  }
  throw last;
}

/** Sign in with email plus password; resolves with a session cookie header. */
export async function signInSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  // Both areas' retry machinery composes: fetchTolerant rides out a 429
  // minute per request, and withThrottleRetry waits out a saturated window
  // when the endpoint still answers 429 (thrown as HTTP 429 below).
  return withThrottleRetry(`sign-in ${email}`, async () => {
    // The credential endpoint is a native form POST guarded by CSRF, so fetch
    // a token first exactly like the sign-in card does, then submit the form.
    const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`);
    if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
    const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
    const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
    if (token === "") throw new Error("[parity] CSRF token response carried no token.");
    const preCookies = cookieHeader(setCookieHeaders(tokenRes));
    const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: token });
    const res = await fetchTolerant(`${apiBase}/auth/sign-in/`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
      body,
      redirect: "manual",
    });
    if (res.status !== 200 && res.status !== 302) {
      throw new Error(`[parity] sign-in failed with HTTP ${res.status} for ${email}.`);
    }
    const header = cookieHeader([...setCookieHeaders(tokenRes), ...setCookieHeaders(res)]);
    if (!header.includes("session-id=")) throw new Error("[parity] sign-in response carried no session cookie.");
    return header;
  });
}

/** Second workspace member the mention scenarios @-mention (NEWFRONT-115). */
export interface ParityMentionMember {
  email: string;
  password: string;
  id: string;
  displayName: string;
}

/** The seeded second member, or a thrown error naming the missing seed step. */
export function requireMentionMember(seed: ParitySeedFacts): ParityMentionMember {
  const member = seed.mentionMember;
  if (member === undefined) {
    throw new Error("[parity] seed facts carry no mentionMember; re-run the stack seed step (see stack/README.md).");
  }
  return member as ParityMentionMember;
}

/** One member suggestion backing the @-mention autocomplete (NEWFRONT-115). */
export interface ParityUserSuggestion {
  id: string;
  displayName: string;
  avatarUrl: string;
}

/**
 * Member search backing the mention suggestion list (NEWFRONT-115): the
 * same entity-search endpoint the old composer queries, so the scenario
 * can prove the server narrows the same way the screen does.
 */
export async function serverUserMentionSuggestions(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  query: string,
  count = 5,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityUserSuggestion[]> {
  const params = new URLSearchParams({
    query,
    query_type: "user_mention",
    project_id: projectId,
    count: String(count),
  });
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/entity-search/?${params}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] entity-search failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { user_mention?: unknown[] };
  const rows = payload.user_mention ?? [];
  return rows.map((row) => {
    const record = row as { member__id?: unknown; member__display_name?: unknown; member__avatar_url?: unknown };
    if (typeof record.member__id !== "string" || typeof record.member__display_name !== "string") {
      throw new Error("[parity] user_mention row carried no member id and display name.");
    }
    return {
      id: record.member__id,
      displayName: record.member__display_name,
      avatarUrl: typeof record.member__avatar_url === "string" ? record.member__avatar_url : "",
    };
  });
}

/** A stored comment with the markup the server kept (NEWFRONT-115). */
export interface ParityStoredComment {
  id: string;
  commentHtml: string;
}

/** Comments stored on an issue, in API order (NEWFRONT-115). */
export async function serverIssueComments(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityStoredComment[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] comments read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; comment_html?: unknown };
    if (typeof record.id !== "string" || typeof record.comment_html !== "string") {
      throw new Error("[parity] comment row carried no string id and comment_html.");
    }
    return { id: record.id, commentHtml: record.comment_html };
  });
}

/** An inbox notification as the server reports it (NEWFRONT-115). */
export interface ParityNotification {
  id: string;
  sender: string;
  entityIdentifier: string;
  entityName: string;
  triggeredBy: string;
  isMentioned: boolean;
}

/**
 * Mention notifications read with an existing session cookie (NEWFRONT-115).
 * The inbox endpoint excludes mention notifications unless asked, so this
 * passes the mentioned filter the app's own mentions tab uses. Poll loops
 * reuse one cookie instead of signing in per iteration, which would trip
 * the stack's auth rate limit under parallel parity runs.
 */
export async function serverNotificationsWithSession(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityNotification[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/?mentioned=true`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notifications read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as {
      id?: unknown;
      sender?: unknown;
      entity_identifier?: unknown;
      entity_name?: unknown;
      triggered_by?: unknown;
      is_mentioned_notification?: unknown;
    };
    if (
      typeof record.id !== "string" ||
      typeof record.sender !== "string" ||
      typeof record.entity_identifier !== "string" ||
      typeof record.entity_name !== "string"
    ) {
      throw new Error("[parity] notification row carried no usable identity fields.");
    }
    return {
      id: record.id,
      sender: record.sender,
      entityIdentifier: record.entity_identifier,
      entityName: record.entity_name,
      triggeredBy: typeof record.triggered_by === "string" ? record.triggered_by : "",
      isMentioned: record.is_mentioned_notification === true,
    };
  });
}

/** Result of the email-first routing check. */
export interface EmailCheckResult {
  existing: boolean;
  status: "MAGIC_CODE" | "CREDENTIAL";
}

/**
 * Raw email-check call; throws carrying the server's error_code on failure.
 * Always anonymous: this endpoint enforces CSRF for session-authenticated
 * callers, so sending a session cookie would 403. Throttle spikes are
 * ridden out by the fetch retry instead.
 */
export async function emailCheck(email: string, apiBase: string = apiBaseFromEnv()): Promise<EmailCheckResult> {
  const res = await fetchTolerant(`${apiBase}/auth/email-check/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email }),
  });
  const payload: unknown = await res.json();
  if (!res.ok) {
    const code = (payload as { error_code?: unknown }).error_code;
    throw new Error(`[parity] email-check failed with HTTP ${res.status} (error_code ${String(code)}).`);
  }
  return payload as EmailCheckResult;
}

/** Instance auth capability flags as the server reports them. */
export async function instanceConfig(
  apiBase: string = apiBaseFromEnv(),
  sessionCookie = ""
): Promise<Record<string, boolean | string | null>> {
  const res = await fetchTolerant(
    `${apiBase}/api/instances/`,
    sessionCookie === "" ? undefined : { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] instance read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { config?: unknown };
  if (typeof payload.config !== "object" || payload.config === null) {
    throw new Error("[parity] instance response carried no config object.");
  }
  return payload.config as Record<string, boolean | string | null>;
}

/**
 * Flip instance configuration keys and return the previous values for every
 * key, so the caller can restore them afterwards. The session must come from
 * adminSignInSession: the license endpoints read the admin session cookie.
 */
export async function patchInstanceConfig(
  patch: Record<string, string>,
  adminSessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, string>> {
  const beforeRes = await fetchTolerant(`${apiBase}/api/instances/configurations/`, {
    headers: { cookie: adminSessionCookie },
  });
  if (!beforeRes.ok) throw new Error(`[parity] instance configurations read failed with HTTP ${beforeRes.status}.`);
  const rows = (await beforeRes.json()) as { key?: unknown; value?: unknown }[];
  const before: Record<string, string> = {};
  for (const key of Object.keys(patch)) {
    const row = rows.find((r) => r.key === key);
    before[key] = typeof row?.value === "string" ? row.value : "";
  }
  const res = await fetchTolerant(`${apiBase}/api/instances/configurations/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: adminSessionCookie },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`[parity] instance configurations patch failed with HTTP ${res.status}.`);
  return before;
}

/** A workspace invitation row as the server reports it. */
export interface InvitationFacts {
  id: string;
  email: string;
  workspaceName: string;
}

/** Invite one address to a workspace; resolves with the created invitation. */
export async function createInvitation(
  workspaceSlug: string,
  email: string,
  ownerSessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<InvitationFacts> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: ownerSessionCookie },
    body: JSON.stringify({ emails: [{ email, role: 15 }] }),
  });
  if (!res.ok) throw new Error(`[parity] invitation create failed with HTTP ${res.status}.`);
  const list = await listInvitations(workspaceSlug, ownerSessionCookie, apiBase);
  const row = list.find((inv) => inv.email.toLowerCase() === email.toLowerCase());
  if (!row) throw new Error(`[parity] invitation for ${email} not found after create.`);
  return row;
}

/** Every pending invitation of a workspace, as the owner sees them. */
export async function listInvitations(
  workspaceSlug: string,
  ownerSessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<InvitationFacts[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, {
    headers: { cookie: ownerSessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] invitation list failed with HTTP ${res.status}.`);
  const rows: unknown = await res.json();
  const list: unknown[] = Array.isArray(rows) ? rows : [];
  return list.map((row) => {
    const record = row as { id?: unknown; email?: unknown; workspace?: { name?: unknown } };
    if (typeof record.id !== "string" || typeof record.email !== "string") {
      throw new Error("[parity] invitation row carried no id/email.");
    }
    const workspaceName = record.workspace?.name;
    if (typeof workspaceName !== "string") throw new Error("[parity] invitation row carried no workspace name.");
    return { id: record.id, email: record.email, workspaceName };
  });
}

/**
 * Mention notifications visible to one user (NEWFRONT-115). Signs in as
 * that user, so the mention scenario can prove the fan-out reached the
 * mentioned member rather than trusting the author's session.
 */
export async function serverNotificationsFor(
  workspaceSlug: string,
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityNotification[]> {
  const sessionCookie = await signInSession(email, password, apiBase);
  return serverNotificationsWithSession(workspaceSlug, sessionCookie, apiBase);
}

/**
 * Ask the server for a one-time code by email. Resolves with the issued key;
 * throws carrying the server's error_code when mail is unconfigured (5025)
 * or attempts are exhausted.
 */
export async function magicGenerate(email: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetchTolerant(`${apiBase}/auth/magic-generate/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email }),
  });
  const payload: unknown = await res.json();
  if (!res.ok) {
    const code = (payload as { error_code?: unknown }).error_code;
    throw new Error(`[parity] magic-generate failed with HTTP ${res.status} (error_code ${String(code)}).`);
  }
  const key = (payload as { key?: unknown }).key;
  if (typeof key !== "string") throw new Error("[parity] magic-generate response carried no key.");
  return key;
}

/** Outcome of posting the code form straight at the server. */
export interface NativeCodeResult {
  status: number;
  location: string;
  sessionCookie: boolean;
}

/** Submit a one-time code the way the code form does; never throws. */
export async function nativeMagicSignIn(
  email: string,
  code: string,
  apiBase: string = apiBaseFromEnv(),
  sessionCookie = ""
): Promise<NativeCodeResult> {
  const tokenRes = await fetchTolerant(
    `${apiBase}/auth/get-csrf-token/`,
    sessionCookie === "" ? undefined : { headers: { cookie: sessionCookie } }
  );
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  const preCookies = cookieHeader(setCookieHeaders(tokenRes));
  const body = new URLSearchParams({ email, code, csrfmiddlewaretoken: token });
  const res = await fetchTolerant(`${apiBase}/auth/magic-sign-in/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
    body,
    redirect: "manual",
  });
  const location = res.headers.get("location") ?? "";
  const gotSessionCookie = cookieHeader([...setCookieHeaders(tokenRes), ...setCookieHeaders(res)]).includes(
    "session-id="
  );
  return { status: res.status, location, sessionCookie: gotSessionCookie };
}

/** The public single-invitation fetch the sign-in header uses (no session). */
export async function singleInvitation(
  workspaceSlug: string,
  invitationId: string,
  apiBase: string = apiBaseFromEnv(),
  sessionCookie = ""
): Promise<InvitationFacts> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/invitations/${invitationId}/join/`,
    sessionCookie === "" ? undefined : { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] single invitation fetch failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; email?: unknown; workspace?: { name?: unknown } };
  if (typeof record.email !== "string") throw new Error("[parity] single invitation carried no email.");
  const workspaceName = record.workspace?.name;
  if (typeof workspaceName !== "string") throw new Error("[parity] single invitation carried no workspace name.");
  return { id: typeof record.id === "string" ? record.id : invitationId, email: record.email, workspaceName };
}

/** Remove an invitation created in-spec. */
export async function deleteInvitation(
  workspaceSlug: string,
  invitationId: string,
  ownerSessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/${invitationId}/`, {
    method: "DELETE",
    headers: { cookie: ownerSessionCookie },
  });
  if (!res.ok && res.status !== 204) throw new Error(`[parity] invitation delete failed with HTTP ${res.status}.`);
}

/**
 * Sign in to the instance admin surface (god-mode); resolves with a session
 * cookie header carrying the admin session. The license endpoints (instance
 * configurations) read a separate admin session cookie, so the app sign-in
 * session is not enough there.
 */
export async function adminSignInSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  const preCookies = cookieHeader(setCookieHeaders(tokenRes));
  const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: token });
  const res = await fetchTolerant(`${apiBase}/api/instances/admins/sign-in/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
    body,
    redirect: "manual",
  });
  if (res.status !== 302) {
    throw new Error(`[parity] admin sign-in failed with HTTP ${res.status} for ${email}.`);
  }
  const header = cookieHeader([...setCookieHeaders(tokenRes), ...setCookieHeaders(res)]);
  if (!header.includes("admin-session-id=")) {
    throw new Error("[parity] admin sign-in response carried no admin session cookie.");
  }
  return header;
}

/** A work-item comment as the server reports it through the history API. */
export interface ServerComment {
  id: string;
  comment_html: string;
  comment_stripped: string;
  actor: string;
  actorDisplayName: string;
  actorIsBot: boolean;
  created_at: string;
  edited_at: string | null;
}

/** Resolve one issue's server UUID by its display name. */
export async function serverIssueIdByName(
  workspaceSlug: string,
  projectId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const issues = await serverIssues(workspaceSlug, projectId, sessionCookie, apiBase);
  const found = issues.find((issue) => issue.name === name);
  if (!found) throw new Error(`[parity] no issue named ${JSON.stringify(name)}.`);
  return found.id;
}

/** Comments on one issue as the server reports them, oldest first. */
export async function composerServerComments(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerComment[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/history/?activity_type=issue-comment`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] comments read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as {
      id?: unknown;
      comment_html?: unknown;
      comment_stripped?: unknown;
      actor?: unknown;
      actor_detail?: unknown;
      created_at?: unknown;
      edited_at?: unknown;
    };
    if (typeof record.id !== "string") throw new Error("[parity] comment row carried no string id.");
    const actor = (record.actor_detail ?? {}) as {
      display_name?: unknown;
      first_name?: unknown;
      is_bot?: unknown;
    };
    return {
      id: record.id,
      comment_html: typeof record.comment_html === "string" ? record.comment_html : "",
      comment_stripped: typeof record.comment_stripped === "string" ? record.comment_stripped : "",
      actor: typeof record.actor === "string" ? record.actor : "",
      actorDisplayName:
        typeof actor.display_name === "string" && actor.display_name.length > 0
          ? actor.display_name
          : typeof actor.first_name === "string"
            ? actor.first_name
            : "",
      actorIsBot: actor.is_bot === true,
      created_at: typeof record.created_at === "string" ? record.created_at : "",
      edited_at: typeof record.edited_at === "string" ? record.edited_at : null,
    };
  });
}

/**
 * Post a comment on one issue as the session owner (NEWFRONT-112, CMT-012
 * bot step). Sends the same create payload the composer posts; the
 * session-authenticated write carries the CSRF token from its cookie.
 * Returns the created comment's server id.
 */
export async function composerServerCreateComment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentHtml: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const csrfMatch = /(?:^|;\s*)csrftoken=([^;]+)/.exec(sessionCookie);
  const csrf = csrfMatch?.[1] ?? "";
  if (csrf === "") throw new Error("[parity] comment create has no CSRF token.");
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/`,
    {
      method: "POST",
      headers: { "content-type": "application/json", cookie: sessionCookie, "X-CSRFToken": csrf },
      body: JSON.stringify({ comment_html: commentHtml }),
    }
  );
  if (!res.ok) throw new Error(`[parity] comment create failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { id?: unknown };
  if (typeof payload.id !== "string") throw new Error("[parity] comment create carried no string id.");
  return payload.id;
}

/**
 * Replace one comment's stored HTML (NEWFRONT-112, CMT-003 untouched-save
 * step). The server stamps an edit time only when the HTML actually
 * differs, so PATCHing the identical body back must leave edited_at null.
 */
export async function serverPatchComment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentId: string,
  commentHtml: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const csrfMatch = /(?:^|;\s*)csrftoken=([^;]+)/.exec(sessionCookie);
  const csrf = csrfMatch?.[1] ?? "";
  if (csrf === "") throw new Error("[parity] comment patch has no CSRF token.");
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`,
    {
      method: "PATCH",
      headers: { "content-type": "application/json", cookie: sessionCookie, "X-CSRFToken": csrf },
      body: JSON.stringify({ comment_html: commentHtml }),
    }
  );
  if (!res.ok) throw new Error(`[parity] comment patch failed with HTTP ${res.status}.`);
}
// ---- Issue detail / peek / widget setup and readback (NEWFRONT-121). ----
// These helpers create and read back the server state that detail, peek,
// and widget scenarios assert against: issues, states, labels, and the
// widget collections. Every call rides out the shared-stack throttle the
// same way the harness does.

/** Minimal issue identity as the list/retrieve endpoints report it. */
export interface IssueFacts {
  id: string;
  sequence_id: number;
  name: string;
}

async function authed(
  workspaceSlug: string,
  path: string,
  sessionCookie: string,
  init?: RequestInit,
  apiBase: string = apiBaseFromEnv()
): Promise<Response> {
  // The seeded stack throttles anonymous calls per minute per IP (shared by
  // every parallel run on this machine), so ride out 429s instead of
  // failing the scenario on a contended stack.
  const backoffMs = [3000, 6000, 12000, 20000, 30000];
  for (let attempt = 0; ; attempt++) {
    const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}${path}`, {
      ...init,
      headers: { ...(init?.headers ?? {}), cookie: sessionCookie },
    });
    if (res.status !== 429 || attempt >= backoffMs.length) return res;
    await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
  }
}

function requireOk(res: Response, what: string): void {
  if (!res.ok) throw new Error(`[parity] ${what} failed with HTTP ${res.status}.`);
}

/** Same throttle-tolerant fetch against `/api/...` paths outside workspaces. */
async function authedApi(
  apiPath: string,
  sessionCookie: string,
  init?: RequestInit,
  apiBase: string = apiBaseFromEnv()
): Promise<Response> {
  const backoffMs = [3000, 6000, 12000, 20000, 30000];
  for (let attempt = 0; ; attempt++) {
    const res = await fetch(`${apiBase}/api${apiPath}`, {
      ...init,
      headers: { ...(init?.headers ?? {}), cookie: sessionCookie },
    });
    if (res.status !== 429 || attempt >= backoffMs.length) return res;
    await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
  }
}

/** Project identifier (e.g. `PAR`) plus the raw record. */
export async function projectFacts(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ identifier: string; record: Record<string, unknown> }> {
  const res = await authed(workspaceSlug, `/projects/${projectId}/`, sessionCookie, undefined, apiBase);
  requireOk(res, "project read");
  const record = (await res.json()) as Record<string, unknown>;
  if (typeof record["identifier"] !== "string") throw new Error("[parity] project row carried no identifier.");
  return { identifier: record["identifier"] as string, record };
}

/** Every (non-deleted) issue of the project with id, sequence, and name. */
export async function issueFacts(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<IssueFacts[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/?per_page=100`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "issues read");
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; sequence_id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.sequence_id !== "number" || typeof record.name !== "string") {
      throw new Error("[parity] issue row carried no id/sequence_id/name.");
    }
    return { id: record.id, sequence_id: record.sequence_id, name: record.name };
  });
}

/** `IDENT-seq` (e.g. `PAR-1`) for the named issue; throws when absent. */
export async function issueSeqForName(
  workspaceSlug: string,
  projectId: string,
  projectIdentifier: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ seq: string; id: string }> {
  const rows = await issueFacts(workspaceSlug, projectId, sessionCookie, apiBase);
  const row = rows.find((r) => r.name === name);
  if (!row) throw new Error(`[parity] no issue named ${JSON.stringify(name)}.`);
  return { seq: `${projectIdentifier}-${row.sequence_id}`, id: row.id };
}

/** Full retrieve of one issue, as the detail page hydrates from. */
export async function fetchIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "issue retrieve");
  return (await res.json()) as Record<string, unknown>;
}

/** Create an issue; resolves with its id and sequence number. */
export async function createIssue(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  extra: Record<string, unknown> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; sequence_id: number }> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name, ...extra }) },
    apiBase
  );
  requireOk(res, "issue create");
  const record = (await res.json()) as { id?: unknown; sequence_id?: unknown };
  if (typeof record.id !== "string" || typeof record.sequence_id !== "number") {
    throw new Error("[parity] issue create response carried no id/sequence_id.");
  }
  return { id: record.id, sequence_id: record.sequence_id };
}

/** Patch one issue; resolves with the updated record. */
export async function patchIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  requireOk(res, "issue patch");
  const raw = await res.text();
  return (raw === "" ? {} : JSON.parse(raw)) as Record<string, unknown>;
}

/** Destroy one issue (used to clean up issues a scenario created). */
export async function deleteIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] issue delete failed with HTTP ${res.status}.`);
}

/** Retrieve one issue without throwing: resolves with the HTTP status plus the record when present. */
export async function issueStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; record: Record<string, unknown> | null }> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (!res.ok) return { status: res.status, record: null };
  return { status: res.status, record: (await res.json()) as Record<string, unknown> };
}

/** Archive one issue; resolves with the archived_at stamp the server reports. */
export async function archiveIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ archived_at: string }> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/archive/`,
    sessionCookie,
    { method: "POST" },
    apiBase
  );
  requireOk(res, "issue archive");
  return (await res.json()) as { archived_at: string };
}

/** Restore one archived issue. */
export async function restoreArchivedIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/archive/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  requireOk(res, "issue restore");
}

/** Retrieve one archived issue without throwing: status plus the record when present. */
export async function archivedIssueStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; record: Record<string, unknown> | null }> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/archive/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (!res.ok) return { status: res.status, record: null };
  return { status: res.status, record: (await res.json()) as Record<string, unknown> };
}

/** Minimal pod identity as the runners pod list reports it. */
export interface PodFacts {
  id: string;
  name: string;
  is_default: boolean;
}

/** Every pod of the project. */
export async function projectPods(
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PodFacts[]> {
  const res = await authedApi(`/runners/pods/?project=${projectId}`, sessionCookie, undefined, apiBase);
  requireOk(res, "pod list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map((row) => ({
    id: row["id"] as string,
    name: row["name"] as string,
    is_default: row["is_default"] === true,
  }));
}

/** Create a pod on the project; resolves with its id and name. */
export async function createPod(
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PodFacts> {
  const res = await authedApi(
    `/runners/pods/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project: projectId, name }),
    },
    apiBase
  );
  requireOk(res, "pod create");
  const row = (await res.json()) as Record<string, unknown>;
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string") {
    throw new Error("[parity] pod create response carried no id/name.");
  }
  return { id: row["id"] as string, name: row["name"] as string, is_default: row["is_default"] === true };
}

/** Delete a pod (best effort: 404 means a sibling already removed it). */
export async function deletePod(
  podId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authedApi(`/runners/pods/${podId}/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] pod delete failed with HTTP ${res.status}.`);
}

/** Project state facts as the state dropdown reports them. */
export interface StateFacts {
  id: string;
  name: string;
  group: string;
}

/** Every state of the project. */
export async function projectStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<StateFacts[]> {
  const res = await authed(workspaceSlug, `/projects/${projectId}/states/`, sessionCookie, undefined, apiBase);
  requireOk(res, "states read");
  const rows: unknown = await res.json();
  const list: unknown[] = Array.isArray(rows) ? rows : [];
  return list.map((row) => {
    const record = row as { id?: unknown; name?: unknown; group?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string") {
      throw new Error("[parity] state row carried no id/name/group.");
    }
    return { id: record.id, name: record.name, group: record.group };
  });
}

/** Create a project state (e.g. a second state so detail scenarios can switch). */
export async function createState(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  group = "started",
  apiBase: string = apiBaseFromEnv()
): Promise<StateFacts> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/states/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, group, color: "#3A3A3A", sequence: 10000 + (Date.now() % 8000) }),
    },
    apiBase
  );
  requireOk(res, "state create");
  const record = (await res.json()) as { id?: unknown; name?: unknown; group?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string") {
    throw new Error("[parity] state create response carried no id/name/group.");
  }
  return { id: record.id, name: record.name, group: record.group };
}

/** Delete a project state created in-spec. */
export async function deleteState(
  workspaceSlug: string,
  projectId: string,
  stateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/states/${stateId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] state delete failed with HTTP ${res.status}.`);
}

/** Project label facts. */
export interface LabelFacts {
  id: string;
  name: string;
}

/** Every label of the project. */
export async function projectLabels(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LabelFacts[]> {
  const res = await authed(workspaceSlug, `/projects/${projectId}/issue-labels/`, sessionCookie, undefined, apiBase);
  requireOk(res, "labels read");
  const rows: unknown = await res.json();
  const list: unknown[] = Array.isArray(rows) ? rows : [];
  return list.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string") {
      throw new Error("[parity] label row carried no id/name.");
    }
    return { id: record.id, name: record.name };
  });
}

/** Create a project label; resolves with its id. */
export async function createProjectLabel(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LabelFacts> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issue-labels/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name }) },
    apiBase
  );
  requireOk(res, "label create");
  const record = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] label create response carried no id/name.");
  }
  return { id: record.id, name: record.name };
}

/** Delete a project label created in-spec. */
export async function deleteProjectLabel(
  workspaceSlug: string,
  projectId: string,
  labelId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issue-labels/${labelId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] label delete failed with HTTP ${res.status}.`);
}

/** True when the session user subscribes to the issue's activity. */
export async function subscriptionStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/subscribe/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "subscription read");
  const payload = (await res.json()) as { subscribed?: unknown };
  if (typeof payload.subscribed !== "boolean") throw new Error("[parity] subscription row carried no boolean.");
  return payload.subscribed;
}

/** Description version rows, newest first as the server reports them. */
export async function descriptionVersions(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/work-items/${issueId}/description-versions/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "description versions read");
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows as Record<string, unknown>[];
}

/** One description version row in full (the list endpoint omits content). */
export async function descriptionVersionDetail(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  versionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/work-items/${issueId}/description-versions/${versionId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "description version read");
  return (await res.json()) as Record<string, unknown>;
}

/**
 * POST a native credential form without a CSRF token; resolves true when the
 * server issues a session anyway. The endpoint answers 200 either way — the
 * session cookie is the success signal, exactly like signInSession reads it.
 */
export async function signInIssuesSessionWithoutCsrf(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(`${apiBase}/auth/sign-in/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ email, password }),
    redirect: "manual",
  });
  await res.arrayBuffer().catch(() => null);
  return cookieHeader(setCookieHeaders(res)).includes("session-id=");
}

/**
 * Create a throwaway account through the same native sign-up POST the
 * sign-up card submits. Used where the seed owner must stay untouched
 * (deactivation, account switching). Resolves with a session cookie header.
 */
export async function createAccountSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const csrf = await fetchCsrf(apiBase);
  const preCookies = cookieHeader(csrf.setCookies);
  // The sign-up card's native form submits both password fields.
  const body = new URLSearchParams({ email, password, confirm_password: password, csrfmiddlewaretoken: csrf.token });
  const res = await fetch(`${apiBase}/auth/sign-up/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302) {
    throw new Error(`[parity] sign-up failed with HTTP ${res.status} for ${email}.`);
  }
  const header = cookieHeader([...preCookies.split("; ").filter((p) => p), ...setCookieHeaders(res)]);
  if (!header.includes("session-id=")) throw new Error("[parity] sign-up response carried no session cookie.");
  return header;
}

/** Whether the session still identifies a user (false after sign-out/expiry/deactivation). */
export async function sessionValid(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  return res.ok;
}

/** Start a CLI device flow; resolves with the user code to approve. */
export async function startDeviceFlow(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetch(`${apiBase}/api/v1/auth/device/start/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({}),
  });
  if (!res.ok) throw new Error(`[parity] device start failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { user_code?: unknown };
  if (typeof payload.user_code !== "string" || payload.user_code === "")
    throw new Error("[parity] device start response carried no user code.");
  return payload.user_code;
}

/** Approve a CLI device code (same call the device-approval page makes). */
export async function approveDeviceCode(
  sessionCookie: string,
  code: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ email: string; workspace: string | null }> {
  const res = await fetch(`${apiBase}/api/v1/auth/device/approve/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ user_code: code }),
  });
  if (!res.ok) throw new Error(`[parity] device approve failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { user_email?: unknown; workspace_slug?: unknown };
  if (typeof payload.user_email !== "string")
    throw new Error("[parity] device approve response carried no user email.");
  return {
    email: payload.user_email,
    workspace: typeof payload.workspace_slug === "string" ? payload.workspace_slug : null,
  };
}

export interface WorkspaceInviteRef {
  id: string;
  token: string;
  email: string;
}

/**
 * Invite addresses to a workspace as an admin session. Resolves with the
 * created invitation ids plus tokens, read back from the workspace
 * invitation list so scenarios can build real emailed links.
 */
export async function createWorkspaceInvites(
  workspaceSlug: string,
  adminSessionCookie: string,
  emails: { email: string; role: number }[],
  apiBase: string = apiBaseFromEnv()
): Promise<WorkspaceInviteRef[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: adminSessionCookie },
    body: JSON.stringify({ emails }),
  });
  if (!res.ok) throw new Error(`[parity] invite create failed with HTTP ${res.status}.`);
  const listed = await workspaceInvitations(workspaceSlug, adminSessionCookie, apiBase);
  return emails.map(({ email }) => {
    const found = listed.find((row) => row.email === email);
    if (!found) throw new Error(`[parity] created invite for ${email} was not listed back.`);
    return found;
  });
}

/** Every invitation row of a workspace, as an admin session sees it. */
export async function workspaceInvitations(
  workspaceSlug: string,
  adminSessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<WorkspaceInviteRef[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, {
    headers: { cookie: adminSessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] invitation list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; token?: unknown; email?: unknown };
    if (typeof record.id !== "string" || typeof record.token !== "string" || typeof record.email !== "string") {
      throw new Error("[parity] invitation row carried no string id/token/email.");
    }
    return { id: record.id, token: record.token, email: record.email };
  });
}

/** Pending invitations of the session owner. */
export async function myInvitations(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; workspaceSlug: string }[]> {
  const res = await fetch(`${apiBase}/api/users/me/workspaces/invitations/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] my-invitations read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; workspace?: { slug?: unknown } };
    if (typeof record.id !== "string" || typeof record.workspace?.slug !== "string") {
      throw new Error("[parity] my-invitation row carried no string id/workspace slug.");
    }
    return { id: record.id, workspaceSlug: record.workspace.slug };
  });
}

/** Answer a single invitation over the API (setup for stale-state variants). */
export async function answerSingleInvitation(
  workspaceSlug: string,
  invitationId: string,
  accepted: boolean,
  token: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/${invitationId}/join/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ accepted, token }),
  });
  if (!res.ok) throw new Error(`[parity] invitation answer failed with HTTP ${res.status}.`);
}

/**
 * Emails of the workspace members as plain strings. Named apart from
 * workspaceMemberEmails, which returns `{ email }` records (NEWFRONT-125).
 */
export async function workspaceMemberEmailStrings(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] member list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { member?: { email?: unknown }; email?: unknown };
    const email = record.member?.email ?? record.email;
    if (typeof email !== "string") throw new Error("[parity] member row carried no string email.");
    return email;
  });
}

export interface UserProfileState {
  is_onboarded: boolean;
  role?: string;
  use_case?: string;
  onboarding_step?: Record<string, boolean>;
  last_workspace_id?: string;
}

/** The session owner's profile flags as the server reports them. */
export async function userProfile(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<UserProfileState> {
  const res = await fetch(`${apiBase}/api/users/me/profile/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  return (await res.json()) as UserProfileState;
}

/** The session owner's user row (name fields) as the server reports them. */
export async function currentUser(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ first_name?: string; last_name?: string; display_name?: string }> {
  const res = await fetch(`${apiBase}/api/users/me/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] user read failed with HTTP ${res.status}.`);
  return (await res.json()) as { first_name?: string; last_name?: string; display_name?: string };
}

/**
 * Mark the session owner onboarded via the marker endpoint only (test
 * setup; the UI path is another row). Named apart from markOnboarded,
 * which takes a FreshUser and also completes every onboarding-step flag.
 */
export async function markSessionOnboarded(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const res = await fetch(`${apiBase}/api/users/me/onboard/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ is_onboarded: true }),
  });
  if (!res.ok) throw new Error(`[parity] onboard mark failed with HTTP ${res.status}.`);
}

/** Create a workspace as the session's user; resolves with its slug. */
export async function createWorkspace(
  sessionCookie: string,
  name: string,
  slug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ name, slug }),
  });
  if (!res.ok) throw new Error(`[parity] workspace create failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { slug?: unknown };
  if (typeof payload.slug !== "string" || payload.slug === "")
    throw new Error("[parity] workspace create response carried no slug.");
  return payload.slug;
}

/** Shell-preference reads and writes for the chrome/tabs scenarios (NEWFRONT-126). */

async function cookieAuthedJson(
  path: string,
  sessionCookie: string,
  init: { method?: string; body?: unknown } = {},
  apiBase: string = apiBaseFromEnv()
): Promise<unknown> {
  const res = await fetch(`${apiBase}${path}`, {
    method: init.method ?? "GET",
    headers: {
      cookie: sessionCookie,
      ...(init.body === undefined ? {} : { "content-type": "application/json" }),
    },
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
  });
  if (!res.ok) throw new Error(`[parity] ${init.method ?? "GET"} ${path} failed with HTTP ${res.status}.`);
  if (res.status === 204) return null;
  return (await res.json()) as unknown;
}

/** Workspace sidebar pin/order map as the server stores it. */
export async function getSidebarPreferences(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, { is_pinned?: boolean; sort_order?: number }>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/sidebar-preferences/`,
    sessionCookie,
    {},
    apiBase
  )) as Record<string, { is_pinned?: boolean; sort_order?: number }>;
}

/** Replace pin/order entries in bulk; used to restore state after a scenario. */
export async function patchSidebarPreferences(
  workspaceSlug: string,
  sessionCookie: string,
  entries: Array<{ key: string; is_pinned: boolean; sort_order: number }>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/sidebar-preferences/`,
    sessionCookie,
    { method: "PATCH", body: entries },
    apiBase
  );
}

/** Workspace-level user properties (project-list display preferences live here). */
export async function getWorkspaceUserProperties(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/user-properties/`,
    sessionCookie,
    {},
    apiBase
  )) as Record<string, unknown>;
}

/** Patch workspace-level user properties; used to restore state after a scenario. */
export async function patchWorkspaceUserProperties(
  workspaceSlug: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/user-properties/`,
    sessionCookie,
    { method: "PATCH", body },
    apiBase
  )) as Record<string, unknown>;
}

/** Per-project per-member tab preferences as the server stores them. */
export async function getProjectUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`,
    sessionCookie,
    {},
    apiBase
  )) as Record<string, unknown>;
}

/** Patch per-project tab preferences; used to restore state after a scenario. */
export async function patchProjectUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`,
    sessionCookie,
    { method: "PATCH", body },
    apiBase
  )) as Record<string, unknown>;
}

/** One project as the server reports it (feature flags live here). */
export async function getProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  return (await cookieAuthedJson(
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    {},
    apiBase
  )) as Record<string, unknown>;
}
// NOTE (NEWFRONT-126 rebase): our WIP createProject/patchProject/deleteProject
// duplicates were removed here — the sibling in-spec trio below (same endpoint
// shapes; create takes name plus identifier) covers the same calls, and our
// two create call sites were repointed to it. getProject stays: no sibling
// equivalent exists.

/** Raw issue rows as the server reports them, in API order. */
async function serverIssueRows(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<unknown[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  return Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
}

/** Names of the project's issues as the server reports them, in API order. */
/** Paginated-or-array list readback shared by the widget collections. */
async function collectionRows(res: Response, what: string): Promise<Record<string, unknown>[]> {
  requireOk(res, what);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows as Record<string, unknown>[];
}

/** Sub-issues of a parent, as the widget reads them. */
export async function subIssues(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/sub-issues/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "sub-issues read");
  // The endpoint returns a dict (`{"sub_issues": [...]}`), not a list.
  const payload: unknown = await res.json();
  if (Array.isArray(payload)) return payload as Record<string, unknown>[];
  if (typeof payload === "object" && payload !== null) {
    const record = payload as Record<string, unknown>;
    for (const key of ["sub_issues", "results"]) {
      if (Array.isArray(record[key])) return record[key] as Record<string, unknown>[];
    }
  }
  throw new Error("[parity] sub-issues read returned an unknown shape.");
}

/** Relations of an issue, flattened across the server's per-type groups. */
export async function issueRelations(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-relation/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "relations read");
  // The endpoint returns a dict keyed by relation type (`blocking`,
  // `blocked_by`, `duplicate`, `relates_to`, `start_after`, ...), not a
  // list, so flatten it with the group key attached to each row.
  const payload = (await res.json()) as Record<string, unknown>;
  const flat: Record<string, unknown>[] = [];
  for (const [group, rows] of Object.entries(payload)) {
    if (Array.isArray(rows))
      for (const row of rows) flat.push({ ...(row as Record<string, unknown>), relation_group: group });
  }
  return flat;
}

/** Add relations: `{relation_type, issues: [ids]}` (see IssueRelationViewSet.create). */
export async function addRelation(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<unknown> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-relation/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  requireOk(res, "relation add");
  return (await res.json()) as unknown;
}

/** Remove one relation edge (both directions, server-side). */
export async function removeRelation(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<unknown> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/remove-relation/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  requireOk(res, "relation remove");
  return (await res.json()) as unknown;
}

/** External links of an issue. */
export async function issueLinks(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-links/`,
    sessionCookie,
    undefined,
    apiBase
  );
  return collectionRows(res, "links read");
}

/** Add an external link `{url, title?}`. */
export async function addLink(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-links/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  requireOk(res, "link add");
  return (await res.json()) as Record<string, unknown>;
}

/** Patch one external link. */
export async function patchLink(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  linkId: string,
  sessionCookie: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-links/${linkId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  requireOk(res, "link patch");
  return (await res.json()) as Record<string, unknown>;
}

/** Delete one external link. */
export async function deleteLink(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  linkId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/issue-links/${linkId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  requireOk(res, "link delete");
}

/** Attachments of an issue. */
export async function issueAttachments(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authedApi(
    `/assets/v2/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/attachments/`,
    sessionCookie,
    undefined,
    apiBase
  );
  return collectionRows(res, "attachments read");
}

/** Comments of an issue, oldest first. */
export async function issueComments(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/comments/`,
    sessionCookie,
    undefined,
    apiBase
  );
  return collectionRows(res, "comments read");
}

/** Recent agent runs visible to the session user, newest first. */
export async function recentRuns(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const res = await authedApi("/runners/runs/?per_page=100", sessionCookie, undefined, apiBase);
  return collectionRows(res, "runs read");
}

export async function serverIssueNames(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const rows = await serverIssueRows(workspaceSlug, projectId, sessionCookie, apiBase);
  return rows.map((row) => {
    const name = (row as { name?: unknown }).name;
    if (typeof name !== "string") throw new Error("[parity] issue row carried no string name.");
    return name;
  });
}

/**
 * Delete one comment (NEWFRONT-115). Scenarios post real comments against
 * the shared scratch stack, so each scenario removes what it posted to
 * leave the seeded issues tidy for the next run.
 */
export async function serverDeleteComment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] comment delete failed with HTTP ${res.status}.`);
}

/** A seeded issue identity for scenarios that must open one issue (NEWFRONT-115). */
export interface ParitySeedIssue {
  id: string;
  name: string;
  sequenceId: number;
}

/** Ids plus names of the project's issues, in API order (NEWFRONT-115). */
export async function serverIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySeedIssue[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown; sequence_id?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.sequence_id !== "number") {
      throw new Error("[parity] issue row carried no string id/name and numeric sequence_id.");
    }
    return { id: record.id, name: record.name, sequenceId: record.sequence_id };
  });
}

/** Project identifier string (e.g. PAR) backing work-item URLs (NEWFRONT-115). */
export async function serverProjectIdentifier(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { identifier?: unknown };
  if (typeof payload.identifier !== "string" || payload.identifier === "") {
    throw new Error("[parity] project row carried no string identifier.");
  }
  return payload.identifier;
}

/**
 * Whether one issue currently resolves to the intake view (NEWFRONT-115).
 * Sibling parity runs triage seed issues into intake, which redirects the
 * detail route away from the comment composer; scenarios skip such issues.
 * Read through the same lite endpoint the browse route uses to decide.
 */
export async function serverIssueIsIntake(
  workspaceSlug: string,
  projectIdentifier: string,
  sequenceId: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/work-items/${projectIdentifier}-${sequenceId}/?lite=1`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] work-item read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { is_intake?: unknown };
  return payload.is_intake === true;
}

/**
 * First seeded issue (by seed name order) that currently opens the regular
 * detail view (NEWFRONT-115). Throws naming the intake conflict when every
 * seeded issue is triaged away, so the failure points at the stack state
 * instead of a missing composer.
 */
export async function serverUsableSeedIssue(
  workspaceSlug: string,
  projectId: string,
  seedIssueNames: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySeedIssue> {
  const issues = await serverIssues(workspaceSlug, projectId, sessionCookie, apiBase);
  const identifier = await serverProjectIdentifier(workspaceSlug, projectId, sessionCookie, apiBase);
  for (const name of seedIssueNames) {
    const candidate = issues.find((i) => i.name === name);
    if (candidate === undefined) continue;
    const intake = await serverIssueIsIntake(workspaceSlug, identifier, candidate.sequenceId, sessionCookie, apiBase);
    if (!intake) return candidate;
  }
  throw new Error(
    "[parity] every seeded issue currently resolves to intake; re-run the stack seed step to reset triage state."
  );
}
// ---------------------------------------------------------------------------
// Workspace-onboarding parity helpers (NEWFRONT-111, rows AUTH-034..043).
//
// Added additively to the NEWFRONT-19 harness (never forking the existing
// helpers above). Onboarding scenarios cannot reuse the seeded owner — it is
// already onboarded — so each scenario mints its own fresh user through the
// same native sign-up endpoint the old sign-up card posts to, seeds just
// enough onboarding progress through the public profile API to land on the
// step under test, and reads workspace / membership / progress state back
// through the same REST API the old app uses. Every created record uses a
// unique suffix so parallel runs and reruns never collide.
// ---------------------------------------------------------------------------

/** A minted user plus everything needed to act as them over API and in a browser. */
export interface FreshUser {
  email: string;
  password: string;
  /** UUID string. */
  userId: string;
  /** `Cookie:` header value for authenticated API calls as this user. */
  cookie: string;
  /** CSRF token value matching the `csrftoken` cookie, for unsafe requests. */
  csrfToken: string;
  apiBase: string;
}

/** A cookie shaped for Playwright's `context.addCookies`. */
export interface BrowserCookie {
  name: string;
  value: string;
  domain: string;
  path: string;
  httpOnly: boolean;
  sameSite: "Lax";
}

/** A short suffix unique enough for parallel runs and reruns. */
export function uniqueSuffix(): string {
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

export function uniqueEmail(prefix = "parity-onb"): string {
  return `${prefix}-${uniqueSuffix()}@example.com`;
}

export function uniqueSlug(prefix = "pw"): string {
  // Slugs allow only '-' and alphanumerics; keep it lowercase and short.
  return `${prefix}-${uniqueSuffix()}`.toLowerCase();
}

function oracleHostFromEnv(): string {
  const raw = process.env["PARITY_ORACLE_URL"] ?? "http://localhost:13000";
  try {
    return new URL(raw).hostname;
  } catch {
    return "localhost";
  }
}

function cookieJar(...groups: string[][]): Map<string, string> {
  // Later groups override earlier ones, so a rotated cookie from a follow-up
  // response wins over the value the first response set.
  const jar = new Map<string, string>();
  for (const group of groups) {
    for (const line of group) {
      const first = (line.split(";", 1)[0] ?? "").trim();
      const eq = first.indexOf("=");
      if (eq > 0) jar.set(first.slice(0, eq), first.slice(eq + 1));
    }
  }
  return jar;
}

function jarToHeader(jar: Map<string, string>): string {
  return [...jar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
}

async function fetchCsrf(apiBase: string): Promise<{ token: string; setCookies: string[] }> {
  // The scratch API throttles auth endpoints under burst load (parallel
  // parity runs share one host). Back off and retry on 429/5xx instead of
  // failing the scenario: the throttle clears within seconds.
  let lastStatus = 0;
  for (let attempt = 0; attempt < 6; attempt++) {
    const res = await fetch(`${apiBase}/auth/get-csrf-token/`);
    lastStatus = res.status;
    if (res.ok) {
      const payload = (await res.json()) as { csrf_token?: unknown };
      const token = typeof payload.csrf_token === "string" ? payload.csrf_token : "";
      if (token === "") throw new Error("[parity] CSRF token response carried no token.");
      return { token, setCookies: setCookieHeaders(res) };
    }
    if (res.status !== 429 && res.status < 500) {
      throw new Error(`[parity] CSRF token fetch failed with HTTP ${res.status}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 2000 * (attempt + 1)));
  }
  throw new Error(`[parity] CSRF token fetch failed with HTTP ${lastStatus} after retries.`);
}

/**
 * Create a brand-new account through the native sign-up form and return the
 * authenticated session for it. Sign-up logs the user in, so the returned
 * cookie is a live session; the user is not onboarded and belongs to no
 * workspace yet.
 */
export async function signUpFreshUser(prefix = "parity-onb", apiBase: string = apiBaseFromEnv()): Promise<FreshUser> {
  const email = uniqueEmail(prefix);
  const password = "Parity-Onb-9x";
  const csrf = await fetchCsrf(apiBase);
  const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: csrf.token });
  const res = await fetch(`${apiBase}/auth/sign-up/`, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      cookie: jarToHeader(cookieJar(csrf.setCookies)),
      referer: `${apiBase}/`,
    },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302) {
    throw new Error(`[parity] sign-up failed with HTTP ${res.status} for ${email}.`);
  }
  const jar = cookieJar(csrf.setCookies, setCookieHeaders(res));
  const cookie = jarToHeader(jar);
  if (!cookie.includes("session-id=")) throw new Error("[parity] sign-up response carried no session cookie.");
  const csrfToken = jar.get("csrftoken") ?? csrf.token;
  const meRes = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie } });
  if (!meRes.ok) throw new Error(`[parity] users/me read failed with HTTP ${meRes.status} after sign-up.`);
  const me = (await meRes.json()) as { id?: unknown };
  const userId = typeof me.id === "string" ? me.id : "";
  return { email, password, userId, cookie, csrfToken, apiBase };
}

/** Cookies for `context.addCookies` so a browser acts as this user without a UI login. */
export function browserCookies(user: FreshUser): BrowserCookie[] {
  const domain = oracleHostFromEnv();
  const jar = cookieJar(user.cookie.split(";").map((pair) => pair.trim()));
  const out: BrowserCookie[] = [];
  for (const name of ["session-id", "csrftoken"]) {
    const value = jar.get(name);
    if (value !== undefined) out.push({ name, value, domain, path: "/", httpOnly: true, sameSite: "Lax" });
  }
  return out;
}

async function authedJson(
  method: "GET" | "POST" | "PATCH",
  path: string,
  user: FreshUser,
  jsonBody?: unknown
): Promise<Response> {
  const headers: Record<string, string> = { cookie: user.cookie };
  if (jsonBody !== undefined) {
    headers["content-type"] = "application/json";
    headers["x-csrftoken"] = user.csrfToken;
    headers["referer"] = `${user.apiBase}/`;
  }
  return fetch(`${user.apiBase}${path}`, {
    method,
    headers,
    body: jsonBody === undefined ? undefined : JSON.stringify(jsonBody),
  });
}

/** The onboarding progress flags plus the onboarded / tour markers. */
export interface OnboardingProgress {
  onboarding_step: {
    profile_complete: boolean;
    workspace_create: boolean;
    workspace_join: boolean;
    workspace_invite: boolean;
  };
  is_onboarded: boolean;
  is_tour_completed: boolean;
}

export async function getOnboardingProgress(user: FreshUser): Promise<OnboardingProgress> {
  const res = await authedJson("GET", "/api/users/me/profile/", user);
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  return (await res.json()) as OnboardingProgress;
}

/** Merge onboarding-step flags on the user's profile (leaves other flags intact). */
export async function setOnboardingStep(
  user: FreshUser,
  steps: Partial<OnboardingProgress["onboarding_step"]>
): Promise<void> {
  const current = await getOnboardingProgress(user);
  const res = await authedJson("PATCH", "/api/users/me/profile/", user, {
    onboarding_step: { ...current.onboarding_step, ...steps },
  });
  if (!res.ok) throw new Error(`[parity] profile onboarding-step update failed with HTTP ${res.status}.`);
}

/** Land a fresh user on the workspace create-or-join step: profile done, nothing else. */
export async function seedAtWorkspaceStep(user: FreshUser): Promise<void> {
  await setOnboardingStep(user, {
    profile_complete: true,
    workspace_create: false,
    workspace_join: false,
    workspace_invite: false,
  });
}

/** Mark the user fully onboarded (all flags plus the onboarded marker). */
export async function markOnboarded(user: FreshUser): Promise<void> {
  await setOnboardingStep(user, {
    profile_complete: true,
    workspace_create: true,
    workspace_join: true,
    workspace_invite: true,
  });
  const res = await authedJson("PATCH", "/api/users/me/onboard/", user, { is_onboarded: true });
  if (!res.ok) throw new Error(`[parity] onboard marker update failed with HTTP ${res.status}.`);
}

export interface CreatedWorkspace {
  id: string;
  slug: string;
  name: string;
  role: number;
}

/** Create a workspace as this user (they become owner, role 20). */
export async function createWorkspaceViaApi(
  user: FreshUser,
  input: { name: string; slug: string; organizationSize?: string }
): Promise<CreatedWorkspace> {
  const res = await authedJson("POST", "/api/workspaces/", user, {
    name: input.name,
    slug: input.slug,
    organization_size: input.organizationSize ?? "2-10",
  });
  if (res.status !== 201 && res.status !== 200) {
    throw new Error(`[parity] workspace create failed with HTTP ${res.status} for slug ${input.slug}.`);
  }
  const ws = (await res.json()) as CreatedWorkspace;
  return ws;
}

/** Point the user's last-visited workspace at a slug (so login lands there). */
export async function setLastWorkspace(user: FreshUser, workspaceId: string): Promise<void> {
  const res = await authedJson("PATCH", "/api/users/me/profile/", user, { last_workspace_id: workspaceId });
  if (!res.ok) throw new Error(`[parity] last-workspace update failed with HTTP ${res.status}.`);
}

/** Slugs of the workspaces the user belongs to. */
export async function userWorkspaceSlugs(user: FreshUser): Promise<string[]> {
  const res = await authedJson("GET", "/api/users/me/workspaces/", user);
  if (!res.ok) throw new Error(`[parity] workspaces list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ slug?: unknown }>;
  return rows.map((r) => (typeof r.slug === "string" ? r.slug : "")).filter((s) => s.length > 0);
}

/** The user's role in a workspace, or null when they are not a member. */
export async function userWorkspaceRole(user: FreshUser, slug: string): Promise<number | null> {
  const res = await authedJson("GET", `/api/workspaces/${slug}/workspace-members/me/`, user);
  if (res.status === 404 || res.status === 403) return null;
  if (!res.ok) throw new Error(`[parity] membership read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { role?: unknown };
  return typeof payload.role === "number" ? payload.role : null;
}

/** Email + role for every member of a workspace (as seen by a member). */
export async function workspaceMembers(user: FreshUser, slug: string): Promise<Array<{ email: string; role: number }>> {
  const res = await authedJson("GET", `/api/workspaces/${slug}/members/`, user);
  if (!res.ok) throw new Error(`[parity] members list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ member?: { email?: unknown }; role?: unknown }>;
  return rows.map((r) => ({
    email: typeof r.member?.email === "string" ? r.member.email : "",
    role: typeof r.role === "number" ? r.role : -1,
  }));
}

/** Send workspace invitations to email+role pairs, as an owner/admin of the workspace. */
export async function sendWorkspaceInvites(
  admin: FreshUser,
  slug: string,
  invites: Array<{ email: string; role: number }>
): Promise<void> {
  const res = await authedJson("POST", `/api/workspaces/${slug}/invitations/`, admin, { emails: invites });
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] bulk invite failed with HTTP ${res.status} for ${slug}.`);
  }
}

/** Emails with a pending invitation to a workspace, as seen by an owner/admin. */
export async function workspacePendingInviteEmails(admin: FreshUser, slug: string): Promise<string[]> {
  const res = await authedJson("GET", `/api/workspaces/${slug}/invitations/`, admin);
  if (!res.ok) throw new Error(`[parity] workspace invitations list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ email?: unknown }>;
  return rows.map((r) => (typeof r.email === "string" ? r.email : "")).filter((e) => e.length > 0);
}

/** Workspace slugs the user has a pending invitation to. */
export async function userInvitationWorkspaceSlugs(user: FreshUser): Promise<string[]> {
  const res = await authedJson("GET", "/api/users/me/workspaces/invitations/", user);
  if (!res.ok) throw new Error(`[parity] invitations list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ workspace?: { slug?: unknown }; workspace_detail?: { slug?: unknown } }>;
  return rows
    .map((r) => {
      const slug = r.workspace_detail?.slug ?? r.workspace?.slug;
      return typeof slug === "string" ? slug : "";
    })
    .filter((s) => s.length > 0);
}

/** Admin emails of the user's pending join requests. */
export async function userJoinRequestAdminEmails(user: FreshUser): Promise<string[]> {
  const res = await authedJson("GET", "/api/users/me/workspaces/join-requests/", user);
  if (!res.ok) throw new Error(`[parity] join-requests list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ admin_email?: unknown; status?: unknown }>;
  return rows.map((r) => (typeof r.admin_email === "string" ? r.admin_email : "")).filter((e) => e.length > 0);
}

/** Create a pending join request to a workspace admin's email, as this user. */
export async function createJoinRequest(user: FreshUser, adminEmail: string): Promise<void> {
  const res = await authedJson("POST", "/api/users/me/workspaces/join-requests/", user, { admin_email: adminEmail });
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] join-request create failed with HTTP ${res.status}.`);
  }
}

/** Whether a workspace slug is available (server slug check). */
export async function slugAvailable(user: FreshUser, slug: string): Promise<boolean> {
  const res = await authedJson("GET", `/api/workspace-slug-check/?slug=${encodeURIComponent(slug)}`, user);
  if (!res.ok) throw new Error(`[parity] slug check failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { status?: unknown };
  return payload.status === true;
}

// NOTE (NEWFRONT-107 rebase onto NEWFRONT-111): NEWFRONT-111 added a narrower
// duplicate instanceConfig(apiBase) helper at this spot. It had no callers
// anywhere in the tree; every live caller uses the superset
// instanceConfig(apiBase, sessionCookie) above (same endpoint, plus optional
// session cookie and throttle retries), so the duplicate is removed here
// instead of forked.

// --- NEWFRONT-113 (rules): comment server-state helpers. Appended
// additively; existing helpers above are untouched.

/** Comment fields the rules specs assert on (plus edited_at for NEWFRONT-122 edit scenarios). */
export interface RulesServerComment {
  id: string;
  access: string;
  labels: string[];
  comment_html: string;
  is_synced: boolean;
  /** Null until the comment is edited; asserted by the NEWFRONT-122 edit scenarios. */
  edited_at: string | null;
}

function rulesCommentOf(row: unknown): RulesServerComment {
  const c = row as Record<string, unknown>;
  if (typeof c["id"] !== "string") throw new Error("[parity] comment row carried no string id.");
  return {
    id: c["id"] as string,
    access: typeof c["access"] === "string" ? c["access"] : "INTERNAL",
    labels: Array.isArray(c["labels"]) ? (c["labels"] as string[]) : [],
    comment_html: typeof c["comment_html"] === "string" ? c["comment_html"] : "",
    is_synced: c["is_synced"] === true,
    edited_at: typeof c["edited_at"] === "string" ? (c["edited_at"] as string) : null,
  };
}

/** One work item's identifiers needed for deep-link assertions. */
export async function serverIssueDetail(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; sequence_id: number; project_identifier: string }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issue read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  const sequenceId = row["sequence_id"];
  if (typeof sequenceId !== "number") throw new Error("[parity] issue row carried no sequence id.");
  const projectRes = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!projectRes.ok) throw new Error(`[parity] project read failed with HTTP ${projectRes.status}.`);
  const project = (await projectRes.json()) as Record<string, unknown>;
  const identifier = project["identifier"];
  if (typeof identifier !== "string") throw new Error("[parity] project row carried no identifier.");
  return { id: issueId, sequence_id: sequenceId, project_identifier: identifier };
}

/** Comments on one work item as the server reports them. */
export async function serverComments(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RulesServerComment[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] comments read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map(rulesCommentOf);
}

/** Raw comment creation: resolves with the HTTP status plus parsed body. */
export async function serverCreateCommentRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  html: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: unknown }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ comment_html: html, comment_json: {} }),
    }
  );
  return { status: res.status, body: await res.json().catch(() => null) };
}

/** Create a comment; throws unless the server answers 201. */
export async function serverCreateComment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  html: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RulesServerComment> {
  const { status, body } = await serverCreateCommentRaw(
    workspaceSlug,
    projectId,
    issueId,
    sessionCookie,
    html,
    apiBase
  );
  if (status !== 201) throw new Error(`[parity] comment create failed with HTTP ${status}: ${JSON.stringify(body)}`);
  return rulesCommentOf(body);
}

/** Raw comment patch: resolves with the HTTP status plus parsed body. */
export async function serverPatchCommentRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentId: string,
  sessionCookie: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: unknown }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(data),
    }
  );
  return { status: res.status, body: await res.json().catch(() => null) };
}

/** Raw comment delete: resolves with the HTTP status (204 carries no body). */
export async function serverDeleteCommentRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: unknown }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  return { status: res.status, body: res.status === 204 ? null : await res.json().catch(() => null) };
}

/** Patch project-level flags (e.g. guest_view_all_features); throws unless 2xx. */
export async function serverPatchProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(data),
  });
  if (!res.ok) throw new Error(`[parity] project patch failed with HTTP ${res.status}.`);
}

/** Unbind the project's git repository; throws unless the server accepts. */
export async function serverUnbindRepository(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] repository unbind failed with HTTP ${res.status}.`);
}

/** Add an emoji reaction to a comment; throws unless the server answers 201. */
export async function serverAddCommentReaction(
  workspaceSlug: string,
  projectId: string,
  commentId: string,
  sessionCookie: string,
  reaction: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/comments/${commentId}/reactions/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ reaction }),
    }
  );
  if (res.status !== 201) throw new Error(`[parity] comment reaction failed with HTTP ${res.status}.`);
}

/** Projects in a workspace (id, name, identifier, anchor). */
export async function serverProjects(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string; identifier: string; anchor: string | null }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] projects read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const r = row as { id?: unknown; name?: unknown; identifier?: unknown; anchor?: unknown };
    if (typeof r.id !== "string" || typeof r.name !== "string" || typeof r.identifier !== "string")
      throw new Error("[parity] project row missed id, name or identifier.");
    return { id: r.id, name: r.name, identifier: r.identifier, anchor: (r.anchor as string | null) ?? null };
  });
}

/** Create a project; returns its UUID. */
export async function serverCreateProject(
  workspaceSlug: string,
  sessionCookie: string,
  name: string,
  identifier: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ name, identifier }),
  });
  const body = (await res.json().catch(() => null)) as { id?: unknown } | null;
  if ((res.status !== 201 && res.status !== 200) || !body || typeof body.id !== "string")
    throw new Error(`[parity] project create failed with HTTP ${res.status}: ${JSON.stringify(body)}`);
  return body.id;
}

/** Delete a project; throws unless the server accepts. */
export async function serverDeleteProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] project delete failed with HTTP ${res.status}.`);
}

/** Delete a work item; throws unless the server accepts. */
export async function serverDeleteIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] issue delete failed with HTTP ${res.status}.`);
}

/** Default (Todo) state UUID of a project. */
export async function serverDefaultStateId(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] states read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const found = rows.find((row) => (row as { default?: unknown }).default === true) ?? rows[0];
  const id = (found as { id?: unknown } | undefined)?.id;
  if (typeof id !== "string") throw new Error("[parity] project has no usable state.");
  return id;
}

/** Create a work item as the signed-in user; returns its UUID. */
export async function serverCreateIssue(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  stateId?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(stateId ? { name, state: stateId } : { name }),
  });
  const body = (await res.json().catch(() => null)) as { id?: unknown } | null;
  if ((res.status !== 201 && res.status !== 200) || !body || typeof body.id !== "string")
    throw new Error(`[parity] issue create failed with HTTP ${res.status}: ${JSON.stringify(body)}`);
  return body.id;
}

// --- NEWFRONT-123 (home): server reads for dashboard assertions. ---
// Same session-cookie style as the base helpers above; every function
// proves what the server stored while the driver proves the screen.

function homeTransient(message: string): boolean {
  return /429|500|502|503|504|fetch failed|ECONNREFUSED|Failed to fetch|CSRF token fetch failed|no session cookie|timed out|TimeoutError|aborted/.test(
    message
  );
}

async function homeAttempt(label: string, run: (signal: AbortSignal) => Promise<Response>): Promise<Response> {
  // One retry pass lives here so every helper tolerates scratch-stack
  // hiccups (restarts, throttling) without each scenario hand-rolling it.
  // Each attempt carries its own timeout: without one, a request the
  // stack accepts but never answers hangs the helper until the test
  // budget dies with no diagnostic.
  let lastError: unknown = null;
  for (let attempt = 1; attempt <= 4; attempt += 1) {
    try {
      const res = await run(AbortSignal.timeout(30_000));
      if ((res.status === 429 || res.status >= 500) && attempt < 4) {
        await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
        continue;
      }
      return res;
    } catch (error) {
      lastError = error;
      const message = error instanceof Error ? error.message : "";
      if (!homeTransient(message) || attempt === 4) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
    }
  }
  throw lastError;
}

async function homeGet<T>(path: string, sessionCookie: string, apiBase: string): Promise<T> {
  const res = await homeAttempt("GET", (signal) =>
    fetch(`${apiBase}${path}`, { headers: { cookie: sessionCookie }, signal })
  );
  if (!res.ok) throw new Error(`[parity] home GET ${path} failed with HTTP ${res.status}.`);
  return (await res.json()) as T;
}

async function homeWrite<T>(
  method: string,
  path: string,
  sessionCookie: string,
  apiBase: string,
  body?: unknown
): Promise<T> {
  const res = await homeAttempt(method, (signal) =>
    fetch(`${apiBase}${path}`, {
      method,
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    })
  );
  if (!res.ok) throw new Error(`[parity] home ${method} ${path} failed with HTTP ${res.status}.`);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return (text === "" ? undefined : JSON.parse(text)) as T;
}

/**
 * Sign in, retrying transient infrastructure failures. The scratch stack
 * is shared by concurrent parity runs: anonymous auth calls are throttled
 * per IP and the API container restarts between runs, so a refused
 * connection, a 429 or a 5xx is noise, not a behavior. Any other failure
 * throws immediately.
 */
export async function signInSessionRetry(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv(),
  attempts: number = 8
): Promise<string> {
  let lastError: unknown = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      // The two-step form dance has no timeout of its own; bound it so a
      // stalled stack retries here instead of burning the test budget.
      return await Promise.race([
        signInSession(email, password, apiBase),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("[parity] sign-in timed out after 60s.")), 60_000)
        ),
      ]);
    } catch (error) {
      lastError = error;
      const message = error instanceof Error ? error.message : "";
      const transient = homeTransient(message);
      if (!transient || attempt === attempts) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
    }
  }
  throw lastError;
}

/** Signed-in user's display name parts plus timezone, as the server knows them. */
export async function serverHomeMe(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ first_name: string; last_name: string; user_timezone: string }> {
  return homeGet("/api/users/me/", sessionCookie, apiBase);
}

/** Signed-in user's profile language and account timezone, as the server knows them. */
export async function serverHomeUser(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ language?: string; user_timezone?: string }> {
  const [profile, account] = await Promise.all([
    homeGet<{ language?: unknown }>("/api/users/me/profile/", sessionCookie, apiBase),
    homeGet<{ user_timezone?: unknown }>("/api/users/me/", sessionCookie, apiBase),
  ]);
  return {
    language: typeof profile.language === "string" ? profile.language : undefined,
    user_timezone: typeof account.user_timezone === "string" ? account.user_timezone : undefined,
  };
}

/** PATCH the signed-in user's record; resolves with the outcome instead of throwing on 4xx. */
export async function serverPatchUser(
  sessionCookie: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ ok: boolean; status: number }> {
  return serverPatchPath(sessionCookie, "/api/users/me/", data, apiBase);
}

/** PATCH the signed-in user's profile (theme, language, tour flags live here). */
export async function serverPatchProfile(
  sessionCookie: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ ok: boolean; status: number }> {
  return serverPatchPath(sessionCookie, "/api/users/me/profile/", data, apiBase);
}

async function serverPatchPath(
  sessionCookie: string,
  path: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ ok: boolean; status: number }> {
  const res = await homeAttempt("PATCH", (signal) =>
    fetch(`${apiBase}${path}`, {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(data),
      signal,
    })
  );
  return { ok: res.ok, status: res.status };
}

/** Whether the seeded user already finished the first-run tour. */
export async function serverTourCompleted(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<boolean> {
  const profile = await homeGet<{ is_tour_completed?: unknown }>("/api/users/me/profile/", sessionCookie, apiBase);
  return profile.is_tour_completed === true;
}

/** Flip the tour flag (used to reset the tour state between scenarios). */
export async function serverSetTourCompleted(
  sessionCookie: string,
  value: boolean,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await homeWrite("PATCH", "/api/users/me/tour-completed/", sessionCookie, apiBase, { is_tour_completed: value });
}

export interface HomeQuickLink {
  id: string;
  title?: string;
  url?: string;
  name?: string;
  link?: string;
}

/** Saved reference links for the workspace, in API order. */
export async function serverQuickLinks(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<HomeQuickLink[]> {
  const payload = await homeGet<unknown>(`/api/workspaces/${workspaceSlug}/quick-links/`, sessionCookie, apiBase);
  if (Array.isArray(payload)) return payload as HomeQuickLink[];
  const results = (payload as { results?: unknown }).results;
  return Array.isArray(results) ? (results as HomeQuickLink[]) : [];
}

/** Create one reference link; resolves with the stored row. */
export async function serverCreateQuickLink(
  workspaceSlug: string,
  sessionCookie: string,
  title: string,
  url: string,
  apiBase: string = apiBaseFromEnv()
): Promise<HomeQuickLink> {
  return homeWrite("POST", `/api/workspaces/${workspaceSlug}/quick-links/`, sessionCookie, apiBase, {
    title,
    url,
  });
}

/** Delete one reference link by id. */
export async function serverDeleteQuickLink(
  workspaceSlug: string,
  sessionCookie: string,
  linkId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await homeWrite("DELETE", `/api/workspaces/${workspaceSlug}/quick-links/${linkId}/`, sessionCookie, apiBase);
}

export interface HomeRecentVisit {
  id: string;
  entity_name?: string;
  entity_data?: { name?: string; id?: string } | null;
}

/** Recent-visit rows for the workspace, optionally narrowed by entity. */
export async function serverRecents(
  workspaceSlug: string,
  sessionCookie: string,
  entity?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<HomeRecentVisit[]> {
  const query = entity === undefined ? "" : `?entity_name=${encodeURIComponent(entity)}`;
  const payload = await homeGet<unknown>(
    `/api/workspaces/${workspaceSlug}/recent-visits/${query}`,
    sessionCookie,
    apiBase
  );
  if (Array.isArray(payload)) return payload as HomeRecentVisit[];
  const results = (payload as { results?: unknown }).results;
  return Array.isArray(results) ? (results as HomeRecentVisit[]) : [];
}

export interface HomeWidgetPref {
  key: string;
  is_enabled?: boolean;
  sort_order?: number;
}

/** Dashboard widget preferences for the workspace. */
export async function serverWidgets(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<HomeWidgetPref[]> {
  const payload = await homeGet<unknown>(`/api/workspaces/${workspaceSlug}/home-preferences/`, sessionCookie, apiBase);
  if (Array.isArray(payload)) return payload as HomeWidgetPref[];
  const results = (payload as { results?: unknown }).results;
  return Array.isArray(results) ? (results as HomeWidgetPref[]) : [];
}

/**
 * Ensure the named widgets are enabled. Crashed runs and sibling runs
 * leave toggles behind, so every scenario that needs a widget normalizes
 * it up front instead of assuming the default state.
 */
export async function serverEnsureWidgets(
  workspaceSlug: string,
  sessionCookie: string,
  widgetKeys: string[],
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  for (const key of widgetKeys) {
    await serverSetWidget(workspaceSlug, sessionCookie, key, { is_enabled: true }, apiBase);
  }
}

/** Patch one widget preference (toggle or reorder payloads). */
export async function serverSetWidget(
  workspaceSlug: string,
  sessionCookie: string,
  widgetKey: string,
  data: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await homeWrite(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/home-preferences/${widgetKey}/`,
    sessionCookie,
    apiBase,
    data
  );
}

/** Create a fresh workspace; resolves with its slug (setup for onboarding scenarios). */
export async function serverCreateWorkspace(
  sessionCookie: string,
  name: string,
  slug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const created = await homeWrite<{ slug?: unknown }>("POST", "/api/workspaces/", sessionCookie, apiBase, {
    name,
    slug,
  });
  if (typeof created?.slug !== "string") throw new Error("[parity] workspace create carried no slug.");
  return created.slug;
}

/** Create a project in a workspace; resolves with its id. */
export async function serverCreateHomeProject(
  workspaceSlug: string,
  sessionCookie: string,
  name: string,
  identifier: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const created = await homeWrite<{ id?: unknown }>(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/`,
    sessionCookie,
    apiBase,
    { name, identifier }
  );
  if (typeof created?.id !== "string") throw new Error("[parity] project create carried no id.");
  return created.id;
}

/**
 * Resolve the seeded project dynamically. Sibling runs reseed the shared
 * scratch stack mid-flight, which rotates the project id, so scenarios
 * pin the project by name instead of trusting a stale seed file.
 */
export async function serverSeedProject(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/details/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  for (const row of rows) {
    const record = row as { id?: unknown; name?: unknown };
    if (record.name === "Parity Project" && typeof record.id === "string") {
      return { id: record.id, name: record.name };
    }
  }
  throw new Error("[parity] seeded Parity Project is missing (stack reseeded without it?).");
}

/** Issue ids plus names for the seeded project (to drive recent visits). */
export async function serverHomeIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] home issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string")
      throw new Error("[parity] home issue row carried no id/name pair.");
    return { id: record.id, name: record.name };
  });
}

/** Module facts for sidebar-module scenarios. */
export interface ModuleFacts {
  id: string;
  name: string;
}

/** Create a project module in-spec (deleted by the scenario). */
export async function createModule(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ModuleFacts> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/modules/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
    },
    apiBase
  );
  requireOk(res, "module create");
  const record = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] module create response carried no id/name.");
  }
  return { id: record.id, name: record.name };
}

/** Delete a project module created in-spec. */
export async function deleteModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/modules/${moduleId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`[parity] module delete failed with HTTP ${res.status}.`);
  }
}

/** Cycle facts for sidebar-cycle scenarios. */
export interface CycleFacts {
  id: string;
  name: string;
}

/** Create a project cycle in-spec (deleted by the scenario). */
export async function createCycle(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CycleFacts> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/cycles/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
    },
    apiBase
  );
  requireOk(res, "cycle create");
  const record = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] cycle create response carried no id/name.");
  }
  return { id: record.id, name: record.name };
}

/** Delete a project cycle created in-spec. */
export async function deleteCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/cycles/${cycleId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`[parity] cycle delete failed with HTTP ${res.status}.`);
  }
}

/** Project facts for the conditional-rows hidden-state scenario. */
export interface ProjectFacts {
  id: string;
  identifier: string;
}

/** Create a scratch project in-spec (views default off; deleted by the scenario). */
export async function createProject(
  workspaceSlug: string,
  sessionCookie: string,
  name: string,
  identifier: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ProjectFacts> {
  const res = await authed(
    workspaceSlug,
    `/projects/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, identifier }),
    },
    apiBase
  );
  requireOk(res, "project create");
  const record = (await res.json()) as { id?: unknown; identifier?: unknown };
  if (typeof record.id !== "string" || typeof record.identifier !== "string") {
    throw new Error("[parity] project create response carried no id/identifier.");
  }
  return { id: record.id, identifier: record.identifier };
}

/** Patch a scratch project's settings (e.g. `{ module_view: true }`). */
export async function patchProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  requireOk(res, "project patch");
  return (await res.json()) as Record<string, unknown>;
}

/**
 * Clear the oracle user's persisted list filters on a project (rich + legacy
 * expressions in user-properties). Sibling filter specs share the user, and
 * a leftover filter hides every list row behind a blank page — the client's
 * local filter state stays empty, so the filtered empty state (and its UI
 * clear action) never renders and only the API clear restores the rows.
 */
export async function clearProjectListFilters(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/user-properties/`,
    sessionCookie,
    {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        rich_filters: {},
        filters: {
          state: null,
          labels: null,
          priority: null,
          assignees: null,
          created_by: null,
          start_date: null,
          subscriber: null,
          state_group: null,
          target_date: null,
        },
      }),
    },
    apiBase
  );
  requireOk(res, "project filters clear");
}

/** Delete a scratch project created in-spec (cascades to its issues). */
export async function deleteProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(workspaceSlug, `/projects/${projectId}/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`[parity] project delete failed with HTTP ${res.status}.`);
  }
}

// ---------------------------------------------------------------------------
// Issues bulk-ops / modal / drafts parity helpers (NEWFRONT-120, rows ISS-108..141).
//
// Added additively to the shared helpers above (never forking them). Where a
// sibling helper already covers the same call with a compatible shape, these
// scenarios reuse it (signInSession, serverIssues, serverIssueNames,
// serverProjects); serverIssueRecord keeps the full detail record because the
// sibling serverIssueDetail returns a narrow identity-only shape.
// ---------------------------------------------------------------------------

/**
 * Fetch with a few retries on rate-limit and transient failures. The
 * parity stack is shared by concurrent oracle runs, so 429s happen; a
 * short backoff is cheaper than a red scenario. Non-retryable statuses
 * return as-is for the caller to judge.
 */
async function fetchShared(input: string, init?: RequestInit, attempts: number = 6): Promise<Response> {
  let last: Response | undefined;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    last = await fetch(input, init);
    if (last.status !== 429 && last.status < 500) return last;
    if (attempt < attempts) await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
  }
  return last as Response;
}

/** Names of the workspace drafts as the server reports them. */
export async function serverDraftNames(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] drafts read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const name = (row as { name?: unknown }).name;
    if (typeof name !== "string") throw new Error("[parity] draft row carried no string name.");
    return name;
  });
}

/** Ids plus names of the project's issues as the server reports them. */
export interface ServerIssue {
  id: string;
  name: string;
}

/** Workspace projects visible to the session (id + name only). */
export type ServerProject = { id: string; name: string };

export async function serverProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerProject & { identifier: string }> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; name?: unknown; identifier?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.identifier !== "string")
    throw new Error("[parity] project carried no string id/name/identifier.");
  return { id: record.id, name: record.name, identifier: record.identifier };
}

/** Raw detail record of one issue; specs pick the fields their row needs. */
export async function serverIssueRecord(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issue detail read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

/** Delete one project issue; test hygiene for issues a scenario created. */
export async function deleteServerIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] issue delete failed with HTTP ${res.status}.`);
}

/** Project states (id, name, group) for setup and assertions. */
export interface ServerState {
  id: string;
  name: string;
  group: string;
}

export async function serverStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerState[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] states read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown; group?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string")
      throw new Error("[parity] state row carried no string id/name/group.");
    return { id: record.id, name: record.name, group: record.group };
  });
}

/** Create a project state; setup for flows that need a completed state. */
export async function createServerState(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  payload: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerState> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`[parity] state create failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; name?: unknown; group?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string")
    throw new Error("[parity] created state carried no string id/name/group.");
  return { id: record.id, name: record.name, group: record.group };
}

/** Delete a project state created for setup. */
export async function deleteServerState(
  workspaceSlug: string,
  projectId: string,
  stateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/${stateId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] state delete failed with HTTP ${res.status}.`);
}

/** Patch one project issue; setup for flows that need a given field value. */
export async function patchServerIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  payload: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(payload),
    },
    3
  );
  if (!res.ok) throw new Error(`[parity] issue patch failed with HTTP ${res.status}.`);
}

/** Archive one project issue through the archive endpoint. */
export async function archiveServerIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    { method: "POST", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] issue archive failed with HTTP ${res.status}.`);
}

/** Restore an archived project issue; hygiene after archive scenarios. */
export async function unarchiveServerIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    { method: "DELETE", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] issue unarchive failed with HTTP ${res.status}.`);
}

/** Delete one workspace draft; test hygiene for drafts a scenario created. */
export async function deleteServerDraft(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] draft delete failed with HTTP ${res.status}.`);
}

/** Cycle/module view flags of one project; setup for flag-gated UI. */
export async function serverProjectViews(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ cycleView: boolean; moduleView: boolean }> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { cycle_view?: unknown; module_view?: unknown };
  return { cycleView: record.cycle_view === true, moduleView: record.module_view === true };
}

/** Ids plus names of the workspace drafts as the server reports them. */
export async function serverDrafts(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIssue[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] drafts read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string")
      throw new Error("[parity] draft row carried no string id/name.");
    return { id: record.id, name: record.name };
  });
}

/** Create a project cycle; setup for flows that need cycle context. */
export async function createServerCycle(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  payload: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIssue> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`[parity] cycle create failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string")
    throw new Error("[parity] created cycle carried no string id/name.");
  return { id: record.id, name: record.name };
}

/** Delete a project cycle created for setup. */
export async function deleteServerCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] cycle delete failed with HTTP ${res.status}.`);
}

/** Create a project module; setup for flows that need module context. */
export async function createServerModule(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  payload: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIssue> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(`[parity] module create failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string")
    throw new Error("[parity] created module carried no string id/name.");
  return { id: record.id, name: record.name };
}

/** Delete a project module created for setup. */
export async function deleteServerModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } },
    3
  );
  if (!res.ok) throw new Error(`[parity] module delete failed with HTTP ${res.status}.`);
}

// ---- Scenario-owned second users (NEWFRONT-121). ----
// Role-gating scenarios sign up a throwaway user, join it to the
// workspace, add it to a scratch project, and tear it all down at the
// end. Every call rides out the shared-stack throttle.

/** Project roles as the member endpoints spell them. */
export const PROJECT_ROLE_GUEST = 5;

/** Register a new user with email plus password (same form POST as the sign-up card). */
export async function signUpUser(email: string, password: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const backoffMs = [3000, 6000, 12000, 20000, 30000];
  for (let attempt = 0; ; attempt++) {
    const tokenRes = await fetch(`${apiBase}/auth/get-csrf-token/`);
    if (tokenRes.status === 429 && attempt < backoffMs.length) {
      await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
      continue;
    }
    if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
    const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
    const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
    if (token === "") throw new Error("[parity] CSRF token response carried no token.");
    const preCookies = cookieHeader(setCookieHeaders(tokenRes));
    const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: token });
    const res = await fetch(`${apiBase}/auth/sign-up/`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
      body,
      redirect: "manual",
    });
    if (res.status === 429 && attempt < backoffMs.length) {
      await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
      continue;
    }
    if (res.status !== 200 && res.status !== 302) {
      throw new Error(`[parity] sign-up failed with HTTP ${res.status} for ${email}.`);
    }
    return;
  }
}

/** Minimal identity of the session user. */
export interface MeFacts {
  id: string;
  email: string;
}

/** The session user as `/users/me/` reports it. */
export async function fetchMe(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<MeFacts> {
  const res = await authedApi(`/users/me/`, sessionCookie, undefined, apiBase);
  requireOk(res, "user read");
  const record = (await res.json()) as { id?: unknown; email?: unknown };
  if (typeof record.id !== "string" || typeof record.email !== "string") {
    throw new Error("[parity] user read carried no id/email.");
  }
  return { id: record.id, email: record.email };
}

/** Mark the session user's onboarding complete so the app shell renders. */
export async function setOnboarded(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const res = await authedApi(
    `/users/me/onboard/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ is_onboarded: true }) },
    apiBase
  );
  requireOk(res, "user onboard");
}

/** Deactivate the session user (best effort: 404 means already gone). */
export async function deactivateUser(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const res = await authedApi(`/users/me/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] user deactivate failed with HTTP ${res.status}.`);
}

/** Invite one email to the workspace with the given role. */
export async function inviteWorkspaceMember(
  workspaceSlug: string,
  sessionCookie: string,
  email: string,
  role: number,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/invitations/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ emails: [{ email, role }] }),
    },
    apiBase
  );
  requireOk(res, "workspace invite");
}

/** Minimal workspace-invite identity (the join token rides along for the owner). */
export interface InviteFacts {
  id: string;
  email: string;
  token: string;
}

/** Every pending workspace invite. */
export async function listWorkspaceInvites(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<InviteFacts[]> {
  const res = await authed(workspaceSlug, `/invitations/`, sessionCookie, undefined, apiBase);
  requireOk(res, "workspace invite list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map((row) => ({
    id: row["id"] as string,
    email: row["email"] as string,
    token: row["token"] as string,
  }));
}

/** Accept a workspace invite as the invited user. */
export async function joinWorkspace(
  workspaceSlug: string,
  inviteId: string,
  token: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/invitations/${inviteId}/join/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ token, accepted: true }),
    },
    apiBase
  );
  requireOk(res, "workspace join");
}

/** Minimal workspace-member identity. */
export interface WorkspaceMemberFacts {
  id: string;
  email: string;
}

/** Every workspace member. */
export async function listWorkspaceMembers(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<WorkspaceMemberFacts[]> {
  const res = await authed(workspaceSlug, `/members/`, sessionCookie, undefined, apiBase);
  requireOk(res, "workspace member list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map((row) => ({
    id: row["id"] as string,
    email: ((row["member"] as Record<string, unknown> | undefined)?.["email"] ?? row["email"]) as string,
  }));
}

/** Remove one workspace member (best effort: 404 means already gone). */
export async function removeWorkspaceMember(
  workspaceSlug: string,
  memberRowId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(workspaceSlug, `/members/${memberRowId}/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] workspace member delete failed with HTTP ${res.status}.`);
}

/** Add members to a project with their roles. */
export async function addProjectMembers(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  members: Array<{ member_id: string; role: number }>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/members/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ members }),
    },
    apiBase
  );
  requireOk(res, "project member add");
}

/** Patch one issue without throwing: resolves with the HTTP status (for 403 probes). */
export async function patchIssueStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  return res.status;
}

/** Delete one issue without throwing: resolves with the HTTP status (for 403 probes). */
export async function deleteIssueStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  return res.status;
}

// ---------------------------------------------------------------------------
// Sidebar + workspace navigation parity helpers (NEWFRONT-125, rows SHELL-046..062).
//
// Added additively to the shared helpers above (never forking them). Cookie-style
// session helpers complement the FreshUser-style ones above: the sidebar scenarios
// sign in through the UI and pass the session cookie explicitly. workspaceMemberEmails
// is the cookie-style sibling of workspaceMembers above (kept separate because the
// signatures are incompatible).
// ---------------------------------------------------------------------------
async function fetchWithRetry(input: string, init?: RequestInit, attempts: number = 8): Promise<Response> {
  let delayMs = 2000;
  let lastError: unknown = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const res = await fetch(input, init);
      if (res.status !== 429 && res.status < 500) return res;
      if (attempt === attempts) return res;
      const retryAfter = Number(res.headers.get("retry-after"));
      await res.arrayBuffer().catch(() => {});
      const waitMs = Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter * 1000 : delayMs;
      await new Promise((resolve) => setTimeout(resolve, waitMs));
    } catch (error) {
      // The scratch API restarts under load; a refused connection is
      // retryable exactly like a 503.
      lastError = error;
      if (attempt === attempts) break;
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
    delayMs = Math.min(delayMs * 2, 30_000);
  }
  throw new Error(`[parity] fetch retry exhausted: ${String(lastError)}`);
}

async function csrfPair(apiBase: string): Promise<{ token: string; cookie: string }> {
  const tokenRes = await fetchWithRetry(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  return { token, cookie: cookieHeader(setCookieHeaders(tokenRes)) };
}

async function apiJson(
  method: string,
  path: string,
  sessionCookie: string,
  body?: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const res = await fetchWithRetry(`${apiBase}${path}`, {
    method,
    headers: {
      cookie: sessionCookie,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await res.text();
  let payload: unknown = null;
  try {
    payload = text === "" ? null : (JSON.parse(text) as unknown);
  } catch {
    payload = text;
  }
  return { status: res.status, payload };
}

/**
 * Single-shot sibling of apiJson: one request, no backoff. For endpoints
 * with a known deterministic failure (see inviteProjectMember), where the
 * retrying call would burn minutes distinguishing a broken endpoint from
 * sloth before the scenario's fallback runs.
 */
async function apiJsonSingle(
  method: string,
  path: string,
  sessionCookie: string,
  body?: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const res = await fetch(`${apiBase}${path}`, {
    method,
    headers: {
      cookie: sessionCookie,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await res.text();
  let payload: unknown = null;
  try {
    payload = text === "" ? null : (JSON.parse(text) as unknown);
  } catch {
    payload = text;
  }
  return { status: res.status, payload };
}

export async function ensureUserSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const { token, cookie } = await csrfPair(apiBase);
  await fetchWithRetry(`${apiBase}/auth/sign-up/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie },
    body: new URLSearchParams({ email, password, csrfmiddlewaretoken: token }),
    redirect: "manual",
  });
  return signInSession(email, password, apiBase);
}

export const WORKSPACE_ROLE_GUEST = 5;

export const WORKSPACE_ROLE_MEMBER = 15;

export async function ensureWorkspaceMember(
  workspaceSlug: string,
  ownerSession: string,
  email: string,
  password: string,
  role: number,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const memberSession = await ensureUserSession(email, password, apiBase);
  const members = await apiJson("GET", `/api/workspaces/${workspaceSlug}/members/`, ownerSession, undefined, apiBase);
  const rows: unknown[] = Array.isArray(members.payload)
    ? members.payload
    : ((members.payload as { results?: unknown[] }).results ?? []);
  const already = rows.some((row) => (row as { member?: { email?: unknown } }).member?.email === email);
  if (already) {
    await onboardUser(memberSession, apiBase);
    return memberSession;
  }
  await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/invitations/`,
    ownerSession,
    { emails: [{ email, role }] },
    apiBase
  );
  const invites = await apiJson("GET", "/api/users/me/workspaces/invitations/", memberSession, undefined, apiBase);
  const pending: unknown[] = Array.isArray(invites.payload)
    ? invites.payload
    : ((invites.payload as { results?: unknown[] }).results ?? []);
  const match = pending.find(
    (row) =>
      ((row as { workspace?: { slug?: unknown } }).workspace?.slug === workspaceSlug ||
        (row as { slug?: unknown }).slug === workspaceSlug) &&
      typeof (row as { id?: unknown }).id === "string"
  ) as { id: string; token?: unknown } | undefined;
  if (match === undefined) throw new Error(`[parity] no pending invitation for ${email}.`);
  const token = typeof match.token === "string" ? match.token : "";
  const joined = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/invitations/${match.id}/join/`,
    memberSession,
    { token, accepted: true },
    apiBase
  );
  if (joined.status !== 200) throw new Error(`[parity] invitation join failed with HTTP ${joined.status}.`);
  await onboardUser(memberSession, apiBase);
  return memberSession;
}

export async function workspaceMemberEmails(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ email: string }[]> {
  const res = await apiJson("GET", `/api/workspaces/${workspaceSlug}/members/`, sessionCookie, undefined, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] workspace members read failed with HTTP ${res.status}.`);
  }
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const r = row as { member?: { email?: unknown } | string; email?: unknown };
    const email = typeof r.member === "string" ? r.email : (r.member?.email ?? r.email);
    return { email: typeof email === "string" ? email : "" };
  });
}

export async function ownerSession(
  seed: { email: string; password: string },
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const session = await signInSession(seed.email, seed.password, apiBase);
  await onboardUser(session, apiBase);
  return session;
}

export async function onboardUser(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const onboarded = await apiJson("PATCH", "/api/users/me/onboard/", sessionCookie, { is_onboarded: true }, apiBase);
  if (onboarded.status !== 200) {
    throw new Error(`[parity] onboard write failed with HTTP ${onboarded.status}.`);
  }
  const toured = await apiJson(
    "PATCH",
    "/api/users/me/tour-completed/",
    sessionCookie,
    { is_tour_completed: true },
    apiBase
  );
  if (toured.status !== 200) {
    throw new Error(`[parity] tour write failed with HTTP ${toured.status}.`);
  }
}

export interface ParityProject {
  id: string;
  name: string;
  identifier: string;
}

export async function ensureProject(
  workspaceSlug: string,
  sessionCookie: string,
  name: string,
  identifier: string,
  apiBase: string = apiBaseFromEnv(),
  extra?: Record<string, unknown>
): Promise<ParityProject> {
  const created = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/`,
    sessionCookie,
    { name, identifier, ...(extra ?? {}) },
    apiBase
  );
  if (created.status === 200 || created.status === 201) {
    const row = created.payload as { id: string; name: string; identifier: string };
    return { id: row.id, name: row.name, identifier: row.identifier };
  }
  const listed = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/details/`,
    sessionCookie,
    undefined,
    apiBase
  );
  const rows: unknown[] = Array.isArray(listed.payload)
    ? listed.payload
    : ((listed.payload as { results?: unknown[] }).results ?? []);
  const match = rows.find((row) => (row as { identifier?: unknown }).identifier === identifier) as
    | ParityProject
    | undefined;
  if (match === undefined) throw new Error(`[parity] project ${identifier} neither created nor listed.`);
  return { id: match.id, name: match.name, identifier: match.identifier };
}

export async function patchUserProperties(
  workspaceSlug: string,
  sessionCookie: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  // Read first: the scratch API throttles write endpoints hardest under
  // shared-stack load, so a guard re-pinning an unchanged value skips the
  // PATCH instead of burning backoff inside a storm. End state is identical.
  const current = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/user-properties/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (current.status !== 200) {
    throw new Error(`[parity] user-properties read failed with HTTP ${current.status}.`);
  }
  const props = (Array.isArray(current.payload) ? current.payload[0] : current.payload) as Record<string, unknown>;
  const drifted = Object.entries(patch).some(([key, value]) => props?.[key] !== value);
  if (!drifted) return props ?? {};
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/user-properties/`,
    sessionCookie,
    patch,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] user properties write failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as Record<string, unknown>;
}

export async function inviteProjectMember(
  workspaceSlug: string,
  ownerSessionCookie: string,
  projectId: string,
  memberSessionCookie: string,
  email: string,
  role: number = WORKSPACE_ROLE_GUEST,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const members = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/members/`,
    ownerSessionCookie,
    undefined,
    apiBase
  );
  const rows: unknown[] = Array.isArray(members.payload)
    ? members.payload
    : ((members.payload as { results?: unknown[] }).results ?? []);
  const memberEmailOf = (row: unknown): unknown => {
    const r = row as { member?: { email?: unknown; id?: unknown } | string; email?: unknown };
    if (typeof r.member === "string") return r.email;
    return r.member?.email ?? r.email;
  };
  const memberIdOf = (row: unknown): unknown => {
    const r = row as { member?: { email?: unknown; id?: unknown } | string };
    if (typeof r.member === "string") return r.member;
    return r.member?.id;
  };
  // Prefer identity over email: project-membership rows carry the member
  // as a bare user id, so the legacy email comparison never matches and
  // every call falls through to the invite POST, which 500s on the shared
  // stack (NEWFRONT-151). The workspace membership embeds the identity to
  // resolve the invitee first.
  let already = false;
  try {
    const wsMembers = await apiJson(
      "GET",
      `/api/workspaces/${workspaceSlug}/members/`,
      ownerSessionCookie,
      undefined,
      apiBase
    );
    const wsRows: unknown[] = Array.isArray(wsMembers.payload)
      ? wsMembers.payload
      : ((wsMembers.payload as { results?: unknown[] }).results ?? []);
    const match = wsRows.find((row) => memberEmailOf(row) === email);
    const userId = match === undefined ? undefined : memberIdOf(match);
    if (typeof userId === "string" && userId.length > 0) {
      already = rows.some((row) => {
        const r = row as { is_active?: unknown };
        return memberIdOf(row) === userId && r.is_active !== false;
      });
    }
  } catch {
    // Fall through to the legacy email check below.
  }
  if (!already) {
    already = rows.some((row) => {
      const r = row as { is_active?: unknown };
      return memberEmailOf(row) === email && r.is_active !== false;
    });
  }
  if (already) return;
  // Fast-fail on the known-broken endpoint (NEWFRONT-151): the invite POST
  // 500s deterministically on the parity stack, and the retrying call would
  // burn ~2 min of backoff per POST before the scenario's fallback runs —
  // timing the scenario out while the bug is open. A single-shot POST
  // separates the deterministic 500 (fail fast into the fallback) from
  // sloth (429: take the retrying path once) and health (2xx: continue
  // into accept). The old delayed re-POST is gone: fetchWithRetry already
  // absorbs intermittent 5xx across its own attempts, so an outer retry
  // only ever bought backoff, never information.
  const invitePath = `/api/workspaces/${workspaceSlug}/projects/${projectId}/invitations/`;
  const inviteBody = { emails: [{ email, role }] };
  const probe = await apiJsonSingle("POST", invitePath, ownerSessionCookie, inviteBody, apiBase);
  const invited =
    probe.status === 429 ? await apiJson("POST", invitePath, ownerSessionCookie, inviteBody, apiBase) : probe;
  if (invited.status !== 200 && invited.status !== 201) {
    throw new Error(`[parity] project invite failed with HTTP ${invited.status}: ${JSON.stringify(invited.payload)}`);
  }
  const invites = await apiJson(
    "GET",
    `/api/users/me/workspaces/${workspaceSlug}/projects/invitations/`,
    memberSessionCookie,
    undefined,
    apiBase
  );
  const pending: unknown[] = Array.isArray(invites.payload)
    ? invites.payload
    : ((invites.payload as { results?: unknown[] }).results ?? []);
  const match = (pending.find((row) => {
    const r = row as { project?: { id?: unknown }; project_id?: unknown; id?: unknown };
    return (
      (typeof r.project?.id === "string" && r.project.id === projectId) ||
      (typeof r.project_id === "string" && r.project_id === projectId)
    );
  }) ?? pending[0]) as { id?: unknown } | undefined;
  if (match === undefined || typeof match.id !== "string") {
    throw new Error(`[parity] no pending project invitation for ${email}.`);
  }
  const joined = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/join/${match.id}/`,
    memberSessionCookie,
    { email, accepted: true },
    apiBase
  );
  if (joined.status !== 200 && joined.status !== 201) {
    throw new Error(
      `[parity] project invite accept failed with HTTP ${joined.status}: ${JSON.stringify(joined.payload)}`
    );
  }
}

export interface ParityPage {
  id: string;
  name: string;
}

export async function ensurePage(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPage> {
  const listed = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (listed.status === 200) {
    const rows: unknown[] = Array.isArray(listed.payload)
      ? listed.payload
      : ((listed.payload as { results?: unknown[] }).results ?? []);
    const match = rows.find((row) => (row as { name?: unknown }).name === name) as ParityPage | undefined;
    if (match !== undefined) return { id: match.id, name: match.name };
  }
  const created = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/`,
    sessionCookie,
    { name },
    apiBase
  );
  if (created.status !== 200 && created.status !== 201) {
    throw new Error(`[parity] page create failed with HTTP ${created.status}: ${JSON.stringify(created.payload)}`);
  }
  const row = created.payload as { id: string; name?: string };
  return { id: row.id, name: row.name ?? name };
}

export async function deletePage(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  pageId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`,
    sessionCookie,
    {},
    apiBase
  );
  const removed = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (removed.status !== 200 && removed.status !== 204) {
    throw new Error(`[parity] page delete failed with HTTP ${removed.status}.`);
  }
}

export interface ParityCycle {
  id: string;
  name: string;
}

export async function ensureCycle(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCycle> {
  const listed = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (listed.status === 200) {
    const rows: unknown[] = Array.isArray(listed.payload)
      ? listed.payload
      : ((listed.payload as { results?: unknown[] }).results ?? []);
    const match = rows.find((row) => (row as { name?: unknown }).name === name) as ParityCycle | undefined;
    if (match !== undefined) return { id: match.id, name: match.name };
  }
  const created = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/`,
    sessionCookie,
    { name },
    apiBase
  );
  if (created.status !== 200 && created.status !== 201) {
    throw new Error(`[parity] cycle create failed with HTTP ${created.status}: ${JSON.stringify(created.payload)}`);
  }
  const row = created.payload as { id: string; name?: string };
  return { id: row.id, name: row.name ?? name };
}

export interface ParityWorkspace {
  id: string;
  slug: string;
  name: string;
}

export async function ensureWorkspace(
  sessionCookie: string,
  name: string,
  slug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspace> {
  const created = await apiJson("POST", "/api/workspaces/", sessionCookie, { name, slug }, apiBase);
  if (created.status === 200 || created.status === 201) {
    const row = created.payload as { id: string; slug: string; name: string };
    return { id: row.id, slug: row.slug, name: row.name };
  }
  const listed = await apiJson("GET", "/api/users/me/workspaces/", sessionCookie, undefined, apiBase);
  const rows: unknown[] = Array.isArray(listed.payload)
    ? listed.payload
    : ((listed.payload as { results?: unknown[] }).results ?? []);
  const match = rows.find((row) => (row as { slug?: unknown }).slug === slug) as ParityWorkspace | undefined;
  if (match === undefined) throw new Error(`[parity] workspace ${slug} neither created nor listed.`);
  return { id: match.id, slug: match.slug, name: match.name };
}

export async function deleteWorkspace(
  sessionCookie: string,
  slug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const removed = await apiJson("DELETE", `/api/workspaces/${slug}/`, sessionCookie, undefined, apiBase);
  if (removed.status !== 200 && removed.status !== 204) {
    throw new Error(`[parity] workspace delete failed with HTTP ${removed.status}.`);
  }
}

export async function patchProjectFlags(
  workspaceSlug: string,
  sessionCookie: string,
  projectId: string,
  patch: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const current = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (current.status !== 200) {
    throw new Error(`[parity] project read failed with HTTP ${current.status}.`);
  }
  const props = current.payload as Record<string, unknown>;
  const drifted = Object.entries(patch).some(([key, value]) => props?.[key] !== value);
  if (!drifted) return;
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    patch,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] project patch failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
}

export interface ParityFavorite {
  id: string;
  entity_type: string;
  entity_identifier: string | null;
  name: string;
  is_folder: boolean;
  parent: string | null;
}

export async function listFavorites(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityFavorite[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/user-favorites/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] favorites read failed with HTTP ${res.status}.`);
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows as ParityFavorite[];
}

export async function createFavorite(
  workspaceSlug: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityFavorite> {
  const res = await apiJson("POST", `/api/workspaces/${workspaceSlug}/user-favorites/`, sessionCookie, body, apiBase);
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] favorite create failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParityFavorite;
}

export async function deleteFavorite(
  workspaceSlug: string,
  sessionCookie: string,
  favoriteId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/user-favorites/${favoriteId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] favorite delete failed with HTTP ${res.status}.`);
  }
}

// --- NEWFRONT-125 review fixes: last-workspace memory, default tab, workspace
// --- logo, intake pending items, member roles. Appended; existing helpers
// --- above are untouched per the shared parity contract.

/** Last-visited workspace id from the session user's profile (null when unset). */
export async function fetchLastWorkspaceId(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await apiJson("GET", "/api/users/me/profile/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  const id = (res.payload as { last_workspace_id?: unknown }).last_workspace_id;
  return typeof id === "string" ? id : null;
}

/** Email plus workspace role for every member of a workspace. */
export interface ParityWorkspaceMember {
  email: string;
  role: number;
}

export async function workspaceMemberRoles(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceMember[]> {
  const res = await apiJson("GET", `/api/workspaces/${workspaceSlug}/members/`, sessionCookie, undefined, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] workspace members read failed with HTTP ${res.status}.`);
  }
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const r = row as { member?: { email?: unknown }; email?: unknown; role?: unknown };
    const email = r.member?.email ?? r.email;
    return {
      email: typeof email === "string" ? email : "",
      role: typeof r.role === "number" ? r.role : -1,
    };
  });
}

/**
 * Set the member's default project tab; resolves with the stored navigation
 * preferences. Other preference branches are carried over untouched.
 */
export async function setProjectDefaultTab(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  tabKey: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const current = await getProjectUserProperties(workspaceSlug, projectId, sessionCookie, apiBase);
  const prefs = (current["preferences"] ?? {}) as Record<string, unknown>;
  const navigation = (prefs["navigation"] ?? {}) as Record<string, unknown>;
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`,
    sessionCookie,
    {
      preferences: {
        ...prefs,
        navigation: { ...navigation, default_tab: tabKey },
      },
    },
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(
      `[parity] project default-tab write failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`
    );
  }
  const stored = (res.payload as { preferences?: unknown }).preferences as Record<string, unknown> | undefined;
  return (stored?.["navigation"] ?? {}) as Record<string, unknown>;
}

/** Set or clear a workspace logo (null/empty clears back to the initial). */
export async function patchWorkspaceLogo(
  workspaceSlug: string,
  sessionCookie: string,
  logo: string | null,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson("PATCH", `/api/workspaces/${workspaceSlug}/`, sessionCookie, { logo }, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] workspace logo write failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
}

/** A pending triage row in a project's intake queue. The id is the underlying
 * work-item id: the intake detail endpoint addresses rows by issue, and the
 * row serializer expands `issue` into the full work item. */
export interface ParityIntakeIssue {
  id: string;
}

/** Pull the underlying work-item id out of a serialized intake row. */
function intakeRowIssueId(row: unknown): string | null {
  const issue = (row as { issue?: unknown }).issue;
  if (typeof issue === "string" && issue.length > 0) return issue;
  const nested = (issue as { id?: unknown } | null | undefined)?.id;
  return typeof nested === "string" && nested.length > 0 ? nested : null;
}

export async function listIntakeIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityIntakeIssue[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-issues/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] intake list failed with HTTP ${res.status}.`);
  }
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows.flatMap((row) => {
    const id = intakeRowIssueId(row);
    return id === null ? [] : [{ id }];
  });
}

/** Submit a work item to a project's intake queue (it lands pending triage). */
export async function createIntakeIssue(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityIntakeIssue> {
  const res = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-issues/`,
    sessionCookie,
    { issue: { name } },
    apiBase
  );
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] intake create failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  const id = intakeRowIssueId(res.payload);
  if (id === null) throw new Error("[parity] intake create response carried no issue id.");
  return { id };
}

/** Materialize a project's intake queue row. Creating a project with the
 * intake flag set stores the flag but no queue row, and the intake endpoints
 * 404 ("Intake not found") until the project PATCH creates it — so PATCH the
 * flag unconditionally (idempotent) before touching the queue. */
export async function ensureProjectIntake(
  workspaceSlug: string,
  sessionCookie: string,
  projectId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    { intake_view: true },
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] intake materialize failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
}

/** Read a workspace's member total as the switcher renders it. The switcher
 * row counts from the workspace's own total field, not from the length of
 * the members list (which can carry extra rows). */
export async function fetchWorkspaceTotalMembers(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await apiJson("GET", `/api/workspaces/${workspaceSlug}/`, sessionCookie, undefined, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] workspace read failed with HTTP ${res.status}.`);
  }
  const total = (res.payload as { total_members?: unknown }).total_members;
  if (typeof total !== "number") throw new Error("[parity] workspace read carried no total_members.");
  return total;
}

export async function deleteIntakeIssue(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  issueId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-issues/${issueId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] intake delete failed with HTTP ${res.status}.`);
  }
}

/** Activity-feed server reads (NEWFRONT-114). The old app merges two split
 * history sources client-side: property-change entries
 * (`activity_type=issue-property`) and comments
 * (`activity_type=issue-comment`). The merged read without `activity_type`
 * 500s (see NEWFRONT-128). Both sides support `created_at__gt` for
 * incremental fetch. Appended additively; existing helpers are untouched. */

/** One history row as the server reports it (property entry or comment). */

export interface HistoryEntry {
  id: string;
  created_at: string;
  activity_type?: unknown;
  field?: unknown;
  verb?: unknown;
  comment?: unknown;
  comment_html?: unknown;
  actor?: unknown;
  [key: string]: unknown;
}

export interface HistoryProbe {
  status: number;
  entries: HistoryEntry[];
}

/** Raw history read; `query` carries the leading `?` (e.g. `?activity_type=issue-property`). */

export async function serverHistory(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  query = "",
  apiBase: string = apiBaseFromEnv()
): Promise<HistoryProbe> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/history/${query}`,
    { headers: { cookie: sessionCookie } }
  );
  if (res.status !== 200) return { status: res.status, entries: [] };
  const payload: unknown = await res.json();
  const entries: HistoryEntry[] = (
    Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? [])
  ) as HistoryEntry[];
  return { status: res.status, entries };
}

/** Delete one comment; resolves with the HTTP status (204 on success). */

export async function serverCleanupIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  email: string,
  password: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const attempt = async (cookie: string): Promise<number> => {
    const comments = await serverHistory(
      workspaceSlug,
      projectId,
      issueId,
      cookie,
      "?activity_type=issue-comment",
      apiBase
    );
    for (const entry of comments.entries) {
      // Best-effort: a comment already removed (reseed race) must not fail
      // cleanup; the issue delete below is the real signal.
      await serverDeleteComment(workspaceSlug, projectId, issueId, entry.id, cookie, apiBase).catch(() => {});
    }
    // Status via the shared deleteIssueStatus (merged #880/#905 contract);
    // our status-returning serverDeleteIssue was dropped to avoid forking it.
    return deleteIssueStatus(workspaceSlug, projectId, issueId, cookie, apiBase);
  };
  const first = await attempt(sessionCookie).catch(() => -1);
  if (first === 204 || first === 404) return;
  const fresh = await signInSessionWithRetry(email, password, apiBase);
  const second = await attempt(fresh);
  if (second !== 204 && second !== 404) throw new Error(`[parity] issue cleanup failed with HTTP ${second}.`);
}

/** Sign in with retries: the scratch API throttles the credential endpoints
 * under concurrent parity runs, so server setup signs in defensively
 * instead of failing the scenario on a 429/502. Untouched existing
 * behavior; this wrapper is additive. */

export async function signInSessionWithRetry(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv(),
  tries = 6
): Promise<string> {
  let last: unknown = null;
  for (let attempt = 1; attempt <= tries; attempt++) {
    try {
      return await signInSession(email, password, apiBase);
    } catch (error) {
      last = error;
      if (attempt < tries) await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
    }
  }
  throw last instanceof Error ? last : new Error(`[parity] sign-in failed after ${tries} tries.`);
}

// ---------------------------------------------------------------------------
// Issue activity, comments and shared property dropdowns (NEWFRONT-122,
// ISS-194–220). Appended; existing helpers above are untouched per the
// shared parity contract. Four names differ from the original NEWFRONT-122
// batches because merged siblings own the plain names: serverCreateIssueFull
// (returns the created row; the merged serverCreateIssue returns the id),
// serverCreateProjectWithFlags (takes project flags), and the lenient
// cleanup deletes serverCleanupIssue/serverCleanupProject (never throw, for
// `finally` blocks on the shared stack; the merged strict deletes throw).
/** Archive an issue through the API (its detail sidebar turns read-only). */
export async function serverArchiveIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] issue archive failed with HTTP ${res.status}.`);
}
/** Link existing issues as sub-issues of a parent through the API. */
export async function serverAddSubIssues(
  workspaceSlug: string,
  projectId: string,
  parentId: string,
  subIssueIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${parentId}/sub-issues/`,
    sessionCookie,
    { sub_issue_ids: subIssueIds }
  );
  if (!res.ok) throw new Error(`[parity] sub-issue link failed with HTTP ${res.status}.`);
}
/** Attach a code-review URL through the API (setup for UI scenarios). */
export async function serverAttachCodeReview(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  url: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerCodeReview> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/code-reviews/`,
    sessionCookie,
    { url }
  );
  if (!res.ok) throw new Error(`[parity] code review attach failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["url"] !== "string")
    throw new Error("[parity] attached review carried no string id/url.");
  return {
    id: rec["id"] as string,
    url: rec["url"] as string,
    title: typeof rec["title"] === "string" ? rec["title"] : "",
  };
}
export async function serverCodeReviews(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerCodeReview[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/code-reviews/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] code reviews read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["url"] !== "string")
      throw new Error("[parity] code review row carried no string id/url.");
    return {
      id: rec["id"] as string,
      url: rec["url"] as string,
      title: typeof rec["title"] === "string" ? rec["title"] : "",
    };
  });
}
/** Reactions on a comment as the server reports them (decimal-string emoji keys). */
export async function serverCommentReactions(
  workspaceSlug: string,
  projectId: string,
  commentId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ reaction: string; actor: string }[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/comments/${commentId}/reactions/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] comment reactions read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    return {
      reaction: typeof rec["reaction"] === "string" ? rec["reaction"] : "",
      actor: typeof rec["actor"] === "string" ? rec["actor"] : "",
    };
  });
}
/**
 * Create a project cycle through the API. Only the creator's scenarios use
 * it (unique `NF122` names); delete it after moving every issue out, since
 * the server refuses to delete cycles that still hold issues.
 */
export async function serverCreateCycle(
  workspaceSlug: string,
  projectId: string,
  name: string,
  startDate: string,
  endDate: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/`,
    sessionCookie,
    { name, description: "", start_date: startDate, end_date: endDate }
  );
  if (!res.ok) throw new Error(`[parity] cycle create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created cycle carried no string id.");
  return rec["id"] as string;
}
/**
 * Create an estimate system with points through the API. Creation alone
 * does not enable the system: the project only shows the Estimate row once
 * its `estimate` field points at the system (see serverSetProjectEstimate).
 */
export async function serverCreateEstimate(
  workspaceSlug: string,
  projectId: string,
  name: string,
  points: string[],
  sessionCookie: string,
  system: "points" | "time" = "points",
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; points: ParityServerEstimatePoint[] }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`,
    sessionCookie,
    {
      estimate: { name, type: system, last_used: true },
      estimate_points: points.map((value) => ({ value })),
    }
  );
  if (!res.ok) throw new Error(`[parity] estimate create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created estimate carried no string id.");
  const rows = Array.isArray(rec["points"]) ? rec["points"] : [];
  return {
    id: rec["id"],
    points: rows.map((row) => {
      const point = row as Record<string, unknown>;
      if (typeof point["id"] !== "string" || typeof point["value"] !== "string")
        throw new Error("[parity] estimate point carried no string id/value.");
      return { id: point["id"], value: point["value"] };
    }),
  };
}
/** Create an issue through the API (isolated setup for UI scenarios). */
export async function serverCreateIssueFull(
  workspaceSlug: string,
  projectId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerIssue> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`,
    sessionCookie,
    { name }
  );
  if (!res.ok) throw new Error(`[parity] issue create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] created issue carried no string id/name.");
  return { id: rec["id"] as string, name: rec["name"] as string };
}
/**
 * Create a project module through the API. Only the creator's scenarios use
 * it (unique `NF122` names); delete it after removing every issue from it.
 */
export async function serverCreateModule(
  workspaceSlug: string,
  projectId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/`,
    sessionCookie,
    { name, description: "" }
  );
  if (!res.ok) throw new Error(`[parity] module create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created module carried no string id.");
  return rec["id"] as string;
}
/**
 * Create a pod on a project. The name suffix allows letters, digits, dots,
 * underscores and dashes only (no spaces); the server prefixes it with the
 * project identifier. Project delete cascades, so specs need no pod
 * teardown of their own.
 */
export async function serverCreatePod(
  projectId: string,
  nameSuffix: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerPod> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/pods/`, sessionCookie, {
    project: projectId,
    name: nameSuffix,
  });
  if (!res.ok) throw new Error(`[parity] pod create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] created pod carried no string id/name.");
  return { id: rec["id"], name: rec["name"], isDefault: rec["is_default"] === true };
}
/**
 * Create a scenario-owned project through the API. Some sidebar rows only
 * render when the project's view flags are on, and the seed project keeps
 * them off — so specs that need those rows mint an isolated project with
 * the flags set, then delete it in a `finally` block. Default states are
 * created server-side, so issues can be added immediately.
 */
export async function serverCreateProjectWithFlags(
  workspaceSlug: string,
  name: string,
  identifier: string,
  flags: ParityProjectFlags,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/projects/`, sessionCookie, {
    name,
    identifier,
    cycle_view: flags.cycleView ?? false,
    module_view: flags.moduleView ?? false,
  });
  if (!res.ok) throw new Error(`[parity] project create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created project carried no string id.");
  const createdId = rec["id"] as string;
  const viewPatch: Record<string, boolean> = {};
  if (flags.inboxView === true) viewPatch["inbox_view"] = true;
  if (flags.issueViewsView === true) viewPatch["issue_views_view"] = true;
  if (Object.keys(viewPatch).length > 0) {
    // The create endpoint ignores these view flags (verified: inbox_view
    // reads back false), so enable them explicitly like estimate activation.
    const patch = await mutateJSON(
      "PATCH",
      `${apiBase}/api/workspaces/${workspaceSlug}/projects/${createdId}/`,
      sessionCookie,
      viewPatch
    );
    if (!patch.ok) throw new Error(`[parity] project view-flags patch failed with HTTP ${patch.status}.`);
  }
  return createdId;
}
/**
 * Create a project state through the API. Only the creator's scenarios use
 * it (unique `NF122` names); delete it after moving every issue back to a
 * seeded state, since the server refuses to delete in-use states.
 */
export async function serverCreateState(
  workspaceSlug: string,
  projectId: string,
  name: string,
  group: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`,
    sessionCookie,
    { name, group, color: "#00aa55", description: "" }
  );
  if (!res.ok) throw new Error(`[parity] state create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created state carried no string id.");
  return rec["id"] as string;
}
/**
 * Create an inbox (intake) issue through the API. The payload nests the
 * work item under `issue` with an in-app source; flat name/title payloads
 * are rejected with "Name is required". Returns both ids: the inbox row id
 * and the nested issue id — detail/delete endpoints take the issue id.
 */
export async function serverCreateInboxIssue(
  workspaceSlug: string,
  projectId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerInboxIssue> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/`,
    sessionCookie,
    { source: "IN_APP", issue: { name } }
  );
  if (!res.ok) throw new Error(`[parity] inbox-issue create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (typeof rec["id"] !== "string" || typeof issue?.["id"] !== "string")
    throw new Error("[parity] created inbox issue carried no inbox/issue ids.");
  return {
    id: rec["id"] as string,
    issueId: issue["id"] as string,
    stateId: typeof issue["state_id"] === "string" ? (issue["state_id"] as string) : "",
  };
}
/** Create a saved project view through the API (layout via display_filters). */
export async function serverCreateView(
  workspaceSlug: string,
  projectId: string,
  name: string,
  layout: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`,
    sessionCookie,
    { name, display_filters: { layout } }
  );
  if (!res.ok) throw new Error(`[parity] view create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created view carried no string id.");
  return rec["id"] as string;
}
/** Delete a scenario-owned cycle (its issues must be moved out first). */
export async function serverDeleteCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] cycle delete failed with HTTP ${res.status}.`);
}
/** Delete a scenario-owned module (its issues must be removed first). */
export async function serverDeleteModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] module delete failed with HTTP ${res.status}.`);
}
export async function serverCleanupProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  try {
    const res = await mutateJSON(
      "DELETE",
      `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
      sessionCookie
    );
    if (!res.ok && res.status !== 204) throw new Error(`[parity] project delete failed with HTTP ${res.status}.`);
  } catch (error) {
    console.log(`[parity] project cleanup skipped: ${error instanceof Error ? error.message : error}`);
  }
}
/** Best-effort inbox-issue cleanup keyed by the nested issue id. */
export async function serverCleanupInboxIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] inbox-issue cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}
/** Best-effort saved-view cleanup; logs instead of throwing. */
export async function serverCleanupView(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] view cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}
/** Delete a project state created for a scenario (issues must be moved off first). */
export async function serverDeleteState(
  workspaceSlug: string,
  projectId: string,
  stateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/${stateId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] state delete failed with HTTP ${res.status}.`);
}
/** Single-issue read through the public REST API. */
export async function serverIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerIssueDetail> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issue read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  const str = (key: string): string => {
    const value = row[key];
    if (typeof value !== "string") throw new Error(`[parity] issue ${issueId} carried no string ${key}.`);
    return value;
  };
  const strOrNull = (key: string): string | null => {
    const value = row[key];
    return value === null || value === undefined ? null : String(value);
  };
  const strArray = (key: string): string[] => {
    const value = row[key];
    return Array.isArray(value) ? value.map(String) : [];
  };
  return {
    id: str("id"),
    name: str("name"),
    state_id: str("state_id"),
    priority: strOrNull("priority"),
    assignee_ids: strArray("assignee_ids"),
    start_date: strOrNull("start_date"),
    target_date: strOrNull("target_date"),
    estimate_point: strOrNull("estimate_point"),
    cycle_id: strOrNull("cycle_id"),
    module_ids: strArray("module_ids"),
    label_ids: strArray("label_ids"),
    archived_at: strOrNull("archived_at"),
    agent_executor: strOrNull("agent_executor"),
    assigned_pod_id: strOrNull("assigned_pod_id"),
  };
}
/** Reactions on an issue as the server reports them. */
export async function serverIssueReactions(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ reaction: string; actor: string }[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/reactions/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] issue reactions read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    return {
      reaction: typeof rec["reaction"] === "string" ? rec["reaction"] : "",
      actor: typeof rec["actor"] === "string" ? rec["actor"] : "",
    };
  });
}
/**
 * Project intake states as the server reports them. The endpoint answers a
 * single state object (fresh projects carry one "Triage" row; POST is 405),
 * so the helper normalizes to a list for the picker assertions.
 */
export async function serverIntakeStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerIntakeState[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-state/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] intake-state read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [payload];
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
      throw new Error("[parity] intake-state row carried no string id/name.");
    return {
      id: rec["id"] as string,
      name: rec["name"] as string,
      group: typeof rec["group"] === "string" ? (rec["group"] as string) : "",
      isDefault: rec["default"] === true,
    };
  });
}
/** One inbox issue as the server reports it, read by the nested issue id. */
export async function serverInboxIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; stateId: string; name: string }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] inbox-issue read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  return {
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    stateId: typeof issue?.["state_id"] === "string" ? (issue["state_id"] as string) : "",
    name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
  };
}
/** One saved view as the server reports it (name plus layout filter). */
export async function serverView(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ name: string; layout: string }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] view read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const filters = rec["display_filters"] as Record<string, unknown> | undefined;
  return {
    name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
    layout: typeof filters?.["layout"] === "string" ? (filters["layout"] as string) : "",
  };
}
/** Inbox issues on a project as the server reports them (issue id plus name). */
export async function serverInboxIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ issueId: string; name: string }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] inbox-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    const issue = rec["issue"] as Record<string, unknown> | undefined;
    return {
      issueId: typeof issue?.["id"] === "string" ? (issue["id"] as string) : "",
      name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
    };
  });
}
/** Saved views on a project as the server reports them (id plus name). */
export async function serverViews(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] views read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    return {
      id: typeof rec["id"] === "string" ? (rec["id"] as string) : "",
      name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
    };
  });
}
/** One cycle as the server reports it (name plus start/end dates). */
export async function serverCycleDetail(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ name: string; startDate: string | null; endDate: string | null; snapshotEmpty: boolean }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] cycle read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const snapshot = rec["progress_snapshot"];
  return {
    name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
    startDate: typeof rec["start_date"] === "string" ? (rec["start_date"] as string) : null,
    endDate: typeof rec["end_date"] === "string" ? (rec["end_date"] as string) : null,
    snapshotEmpty: snapshot === null || snapshot === undefined || JSON.stringify(snapshot) === "{}",
  };
}
/** Current user as the server reports them. */
export async function serverMe(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; displayName: string }> {
  const res = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] users/me read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["display_name"] !== "string")
    throw new Error("[parity] users/me carried no string id/display_name.");
  return { id: rec["id"] as string, displayName: rec["display_name"] as string };
}
/**
 * Patch an issue through the public REST API (state moves, priority changes).
 *
 * WARNING (NEWFRONT-160): patching state_id to In Progress, In Review, or
 * In Test answers 204 but silently keeps the default state instead — the
 * same happens through UI drags. Fixtures must only target verified
 * states (Backlog, Todo, Done, and created states); Cancelled is untested.
 */
export async function serverPatchIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  patch: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    patch
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] issue patch failed with HTTP ${res.status}.`);
}
/** Post a comment through the API (setup for UI scenarios, never the assertion). */
export async function serverPostComment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  commentHtml: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerComment> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/comments/`,
    sessionCookie,
    { comment_html: commentHtml }
  );
  if (!res.ok) throw new Error(`[parity] comment create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return {
    id: rec["id"] as string,
    comment_html: rec["comment_html"] as string,
    actor: typeof rec["actor"] === "string" ? rec["actor"] : "",
    access: typeof rec["access"] === "string" ? rec["access"] : "",
    labels: [],
    edited_at: null,
  };
}
/** Project cycles as the server reports them, in API order. */
export async function serverProjectCycles(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerCycle[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] cycles read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string" || typeof rec["status"] !== "string")
      throw new Error("[parity] cycle row carried no string id/name/status.");
    return { id: rec["id"] as string, name: rec["name"] as string, status: rec["status"] as string };
  });
}
/**
 * Project members as the server reports them (user id plus numeric role).
 * The assignee picker hides role-5 guests, so scenarios filter on `role`.
 */
export async function serverProjectMembers(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ userId: string; role: number }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project members read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    if (typeof row["member"] !== "string" || typeof row["role"] !== "number")
      throw new Error("[parity] project member carried no string member/numeric role.");
    return { userId: row["member"] as string, role: row["role"] as number };
  });
}
/** Project modules as the server reports them, in API order. */
export async function serverProjectModules(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerModule[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] modules read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
      throw new Error("[parity] module row carried no string id/name.");
    return { id: rec["id"] as string, name: rec["name"] as string };
  });
}
/** Pods of a project as the server reports them, in API order. */
export async function serverProjectPods(
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerPod[]> {
  const res = await fetch(`${apiBase}/api/runners/pods/?project=${projectId}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] pods read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    const { id, name, is_default } = row;
    if (typeof id !== "string" || typeof name !== "string" || typeof is_default !== "boolean")
      throw new Error("[parity] pod row carried no string id/name or boolean is_default.");
    return { id, name, isDefault: is_default };
  });
}
/** Project states as the server reports them, in API order. */
export async function serverProjectStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerState[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] states read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    const { id, name, group } = row;
    if (typeof id !== "string" || typeof name !== "string" || typeof group !== "string")
      throw new Error("[parity] state row carried no string id/name/group.");
    return { id, name, group };
  });
}
/**
 * Publish the project's public board through the API (anchors the project).
 * `owned` is false when a sibling run published first and this call reused
 * their board; only the owner must unpublish it again.
 */
export async function serverPublishBoard(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ boardId: string; owned: boolean }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/`,
    sessionCookie,
    { is_comments_enabled: true, is_reactions_enabled: true, is_votes_enabled: false }
  );
  if (res.ok) {
    const rec = (await res.json()) as Record<string, unknown>;
    if (typeof rec["id"] !== "string") throw new Error("[parity] published board carried no string id.");
    return { boardId: rec["id"] as string, owned: true };
  }
  // A sibling run may have published first: reuse the existing board.
  if (res.status === 409 || res.status === 400) {
    const existing = await fetch(
      `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/`,
      { headers: { cookie: sessionCookie } }
    );
    if (existing.ok) {
      const rec = (await existing.json()) as Record<string, unknown>;
      if (typeof rec["id"] === "string") return { boardId: rec["id"] as string, owned: false };
    }
  }
  throw new Error(`[parity] board publish failed with HTTP ${res.status}.`);
}
/**
 * Point a project at an estimate system (enables the Estimate sidebar row).
 * Project delete cascades, so specs need no estimate teardown of their own.
 */
export async function serverSetProjectEstimate(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    { estimate: estimateId }
  );
  if (!res.ok) throw new Error(`[parity] project estimate patch failed with HTTP ${res.status}.`);
}
/** Undo an API archive so teardown leaves no archived issue behind. */
export async function serverUnarchiveIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 404) throw new Error(`[parity] issue unarchive failed with HTTP ${res.status}.`);
}
/** Best-effort board removal; logs instead of throwing so teardown never fails a scenario. */
export async function serverUnpublishBoard(
  workspaceSlug: string,
  projectId: string,
  boardId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/${boardId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204 && res.status !== 404)
    console.log(`[parity] board cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}
/** Workspace members as the server reports them (membership id, user id, display name). */
export async function serverWorkspaceMembers(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ membershipId: string; userId: string; displayName: string }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace members read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    const member = row["member"] as Record<string, unknown>;
    if (
      typeof row["id"] !== "string" ||
      typeof member?.["id"] !== "string" ||
      typeof member?.["display_name"] !== "string"
    )
      throw new Error("[parity] workspace member carried no id/user/display_name.");
    return {
      membershipId: row["id"] as string,
      userId: member["id"] as string,
      displayName: member["display_name"] as string,
    };
  });
}
/** Plain text out of stored comment HTML for readable assertions. */
export function commentText(commentHtml: string): string {
  return commentHtml
    .replace(/<[^>]*>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();
}
/**
 * A project identifier the server accepts (short uppercase alphanumerics)
 * that no other scenario run is using. Identifiers must be unique per
 * workspace, so every scenario-owned project mints its own.
 */
export function parityProjectIdentifier(prefix: string): string {
  const stamp = Date.now().toString(36).slice(-4).toUpperCase();
  const salt = Math.floor(Math.random() * 1296)
    .toString(36)
    .toUpperCase()
    .padStart(2, "0");
  return `${prefix}${stamp}${salt}`.replace(/[^A-Z0-9]/g, "").slice(0, 12);
}
/** View flags a scenario-owned project needs for its dropdown rows. */
export interface ParityProjectFlags {
  cycleView?: boolean;
  moduleView?: boolean;
  /** Enable the intake/inbox view (applied via PATCH: create ignores it). */
  inboxView?: boolean;
  /** Enable saved project views (applied via PATCH like inbox_view). */
  issueViewsView?: boolean;
}
/** Code-review links on an issue as the server reports them. */
export interface ParityServerCodeReview {
  id: string;
  url: string;
  title: string;
}
/** One issue comment as the server reports it. */
export interface ParityServerComment {
  id: string;
  comment_html: string;
  actor: string;
  access: string;
  labels: string[];
  edited_at: string | null;
}
/** One project cycle as the server reports it (status derives from its dates). */
export interface ParityServerCycle {
  id: string;
  name: string;
  status: string;
}
/**
 * Delete a scenario-owned project (cascades to its issues, cycles and
 * modules server-side). Best-effort so teardown never fails a scenario.
 */
/** One estimate point as the server reports it. */
export interface ParityServerEstimatePoint {
  id: string;
  value: string;
}
/** One issue row as the server reports it (id plus name for drill-down). */
export interface ParityServerIssue {
  id: string;
  name: string;
}
/** One issue as the server reports it (common property fields for dropdown assertions). */
export interface ParityServerIssueDetail {
  id: string;
  name: string;
  state_id: string;
  priority: string | null;
  assignee_ids: string[];
  start_date: string | null;
  target_date: string | null;
  estimate_point: string | null;
  cycle_id: string | null;
  module_ids: string[];
  label_ids: string[];
  archived_at: string | null;
  agent_executor: string | null;
  assigned_pod_id: string | null;
}
/** One project module as the server reports it. */
export interface ParityServerModule {
  id: string;
  name: string;
}
/** One pod as the server reports it. */
export interface ParityServerPod {
  id: string;
  name: string;
  isDefault: boolean;
}
/** One project state as the server reports it. */
export interface ParityServerState {
  id: string;
  name: string;
  group: string;
}
/** One intake state as the server reports it. */
export interface ParityServerIntakeState {
  id: string;
  name: string;
  group: string;
  isDefault: boolean;
}
/** One inbox issue as the server reports it on create (inbox id + nested issue id). */
export interface ParityServerInboxIssue {
  id: string;
  issueId: string;
  stateId: string;
}
/** CSRF token out of a signed-in cookie jar (empty when the jar has none). */
function csrfFromCookie(sessionCookie: string): string {
  for (const pair of sessionCookie.split(";")) {
    const [key, ...rest] = pair.trim().split("=");
    if (key === "csrftoken") return rest.join("=");
  }
  return "";
}
/** POST/DELETE/PATCH/PUT JSON with the session jar plus a CSRF header when present. */
async function mutateJSON(
  method: "POST" | "PATCH" | "DELETE" | "PUT",
  url: string,
  sessionCookie: string,
  body?: unknown
): Promise<Response> {
  const csrf = csrfFromCookie(sessionCookie);
  return fetch(url, {
    method,
    headers: {
      cookie: sessionCookie,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...(csrf === "" ? {} : { "X-CSRFToken": csrf, referer: url }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}
/** A project label row as the issue-labels endpoints return it. */
export interface ParityServerLabel {
  id: string;
  name: string;
  color: string;
  parent: string | null;
  sortOrder: number;
  projectId: string;
}
function labelOf(row: Record<string, unknown>): ParityServerLabel {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string")
    throw new Error("[parity] label row carried no string id/name.");
  const parent = row["parent"];
  const sortOrder = row["sort_order"];
  const projectId = row["project_id"];
  return {
    id: row["id"],
    name: row["name"],
    color: typeof row["color"] === "string" ? row["color"] : "",
    parent: typeof parent === "string" ? parent : null,
    sortOrder: typeof sortOrder === "number" ? sortOrder : 0,
    projectId: typeof projectId === "string" ? projectId : "",
  };
}
/** All labels on a project, in server order. */
export async function serverLabels(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerLabel[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] label list failed with HTTP ${res.status}.`);
  return ((await res.json()) as Record<string, unknown>[]).map(labelOf);
}
/** One label by id; null when the server has no such label (deleted). */
export async function serverLabelOrNull(
  workspaceSlug: string,
  projectId: string,
  labelId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerLabel | null> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/${labelId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`[parity] label read failed with HTTP ${res.status}.`);
  return labelOf((await res.json()) as Record<string, unknown>);
}
/** Create a project label; the server assigns sort_order. */
export async function serverCreateLabel(
  workspaceSlug: string,
  projectId: string,
  name: string,
  color: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerLabel> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/`,
    sessionCookie,
    { name, color }
  );
  if (!res.ok) throw new Error(`[parity] label create failed with HTTP ${res.status}.`);
  return labelOf((await res.json()) as Record<string, unknown>);
}
/** Strict label PATCH (rename/recolor/reparent/reorder); throws on failure. */
export async function serverPatchLabel(
  workspaceSlug: string,
  projectId: string,
  labelId: string,
  data: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerLabel> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/${labelId}/`,
    sessionCookie,
    data
  );
  if (!res.ok) throw new Error(`[parity] label patch failed with HTTP ${res.status}.`);
  return labelOf((await res.json()) as Record<string, unknown>);
}
/** Best-effort label cleanup; logs instead of throwing. */
export async function serverCleanupLabel(
  workspaceSlug: string,
  projectId: string,
  labelId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/${labelId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] label cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}
/** Replace an issue's label set (the attach field is `label_ids`). */
export async function serverSetIssueLabels(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  labelIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { label_ids: labelIds }
  );
  if (!res.ok) throw new Error(`[parity] issue label attach failed with HTTP ${res.status}.`);
}

// --- Cross-cutting permission + cycle-transfer helpers (NEWFRONT-122,
// --- ISS-221–225). Observed on the running old app: project member-add
// --- takes {"members":[{"member_id","role"}]} (a "member" key 404s);
// --- guests read issues only when the project sets
// --- guest_view_all_features; invitation acceptance goes through
// --- /api/users/me/workspaces/invitations/ (the per-invitation join
// --- endpoint 200s "not accepted"); cycle transferable counts live
// --- only on the progress endpoint; transfer moves the issue's cycle.
// --- Signup, invite-create and member-listing reuse the auth area's
// --- signUpFreshUser/createInvitation/serverWorkspaceMembers.

/** Id of the user holding `sessionCookie`. */
export async function serverUserId(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] users/me read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  if (typeof row["id"] !== "string") throw new Error("[parity] users/me carried no string id.");
  return row["id"];
}

/** Accept workspace invitations as the invitee (204 on success). */
export async function serverAcceptWorkspaceInvitations(
  invitationIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("POST", `${apiBase}/api/users/me/workspaces/invitations/`, sessionCookie, {
    invitations: invitationIds,
  });
  if (!res.ok) throw new Error(`[parity] invitation accept failed with HTTP ${res.status}.`);
}

/**
 * Complete onboarding for the session holder (the app's own finish call),
 * so a provisioned member lands on the workspace instead of the setup
 * funnel when they sign in through the browser.
 */
export async function serverCompleteOnboarding(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("PATCH", `${apiBase}/api/users/me/onboard/`, sessionCookie, {
    is_onboarded: true,
  });
  if (!res.ok) throw new Error(`[parity] onboard completion failed with HTTP ${res.status}.`);
}

/** Lenient workspace-membership removal for finally-blocks. */
export async function serverCleanupWorkspaceMember(
  workspaceSlug: string,
  membershipId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/members/${membershipId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] workspace member cleanup DELETE returned HTTP ${res.status}; leaving it.`);
}

/**
 * Register a fresh member (role 15), invite them to the workspace and
 * accept as them. Returns credentials plus the signed-in session. The
 * caller removes the workspace membership in cleanup (server-side via
 * serverWorkspaceMembers + serverCleanupWorkspaceMember); the user row
 * itself stays but belongs to nothing.
 */
export async function serverProvisionWorkspaceMember(
  workspaceSlug: string,
  emailPrefix: string,
  ownerSession: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ userId: string; session: string; email: string; password: string }> {
  const fresh = await signUpFreshUser(emailPrefix, apiBase);
  const invitation = await createInvitation(workspaceSlug, fresh.email, ownerSession, apiBase);
  await serverAcceptWorkspaceInvitations([invitation.id], fresh.cookie, apiBase);
  return { userId: fresh.userId, session: fresh.cookie, email: fresh.email, password: fresh.password };
}

/** Add existing workspace users to a project by user id and numeric role. */
export async function serverAddProjectMembers(
  workspaceSlug: string,
  projectId: string,
  adds: { memberId: string; role: number }[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/`,
    sessionCookie,
    { members: adds.map((add) => ({ member_id: add.memberId, role: add.role })) }
  );
  if (!res.ok) throw new Error(`[parity] project member add failed with HTTP ${res.status}.`);
}

/** PATCH a cycle (dates drive its computed status). */
export async function serverPatchCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  data: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`,
    sessionCookie,
    data
  );
  if (!res.ok) throw new Error(`[parity] cycle patch failed with HTTP ${res.status}.`);
}

/** Attach issues to a cycle. */
export async function serverAttachCycleIssues(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  issueIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`,
    sessionCookie,
    { issues: issueIds }
  );
  if (!res.ok) throw new Error(`[parity] cycle attach failed with HTTP ${res.status}.`);
}

/** Cycle progress counts (the only source of transferable breakdowns). */
export async function serverCycleProgress(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ backlog: number; unstarted: number; started: number; total: number }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/progress/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle progress read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  const num = (key: string): number => (typeof row[key] === "number" ? (row[key] as number) : 0);
  return {
    backlog: num("backlog_issues"),
    unstarted: num("unstarted_issues"),
    started: num("started_issues"),
    total: num("total_issues"),
  };
}

/** API base URL scenarios need when they build request URLs themselves. */
export function parityApiBase(): string {
  return apiBaseFromEnv();
}

/**
 * Fire one API request without throwing, for refusal-path assertions
 * (a 403 is the expected behavior, not a helper failure).
 */
export async function serverRequestStatus(
  method: "GET" | "POST" | "PATCH" | "DELETE",
  url: string,
  sessionCookie: string,
  body?: unknown
): Promise<{ status: number; bodyText: string }> {
  const csrf = csrfFromCookie(sessionCookie);
  const res = await fetch(url, {
    method,
    headers: {
      cookie: sessionCookie,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...(csrf === "" ? {} : { "X-CSRFToken": csrf, referer: url }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}
/** Best-effort issue cleanup; logs instead of throwing so teardown never fails a scenario. */
export async function serverCleanupIssueWithSession(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] issue cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}

/** Issue keys (name plus per-project sequence) for building detail addresses. */
export async function serverIssueKeys(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Array<{ name: string; sequence_id: number }>> {
  const rows = await serverIssueRows(workspaceSlug, projectId, sessionCookie, apiBase);
  return rows.flatMap((row) => {
    const record = row as { name?: unknown; sequence_id?: unknown };
    return typeof record.name === "string" && typeof record.sequence_id === "number"
      ? [{ name: record.name, sequence_id: record.sequence_id }]
      : [];
  });
}

// --- Issues filters and display options (NEWFRONT-119): per-user filter
// --- records, saved-view CRUD, and project feature flags.

/** Issue-filter state as the server stores it per user per entity. */
export interface IssueUserProperties {
  filters: Record<string, string[] | null>;
  display_filters: Record<string, unknown>;
  display_properties: Record<string, boolean>;
  rich_filters: Record<string, unknown>;
}

/** The project's per-user filter record for the session owner. */
export async function serverUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<IssueUserProperties> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] user-properties read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as {
    filters?: Record<string, string[] | null>;
    display_filters?: Record<string, unknown>;
    display_properties?: Record<string, boolean>;
    rich_filters?: Record<string, unknown>;
  };
  return {
    filters: payload.filters ?? {},
    display_filters: payload.display_filters ?? {},
    display_properties: payload.display_properties ?? {},
    rich_filters: payload.rich_filters ?? {},
  };
}

const SEED_NULL_FILTERS = [
  "state",
  "labels",
  "priority",
  "assignees",
  "created_by",
  "start_date",
  "subscriber",
  "state_group",
  "target_date",
];

/** PATCH the per-user filter record (session cookie plus CSRF header). */
async function patchIssueUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string
): Promise<void> {
  const csrf = sessionCookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("csrftoken="))
    ?.slice("csrftoken=".length);
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`, {
    method: "PATCH",
    headers: {
      cookie: sessionCookie,
      "content-type": "application/json",
      ...(csrf === undefined ? {} : { "X-CSRFToken": csrf }),
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`[parity] user-properties write failed with HTTP ${res.status}.`);
}

/** Restore the seeded filter record so scenarios never leak state into each other. */
export async function resetIssueUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await patchIssueUserProperties(
    workspaceSlug,
    projectId,
    sessionCookie,
    {
      filters: Object.fromEntries(SEED_NULL_FILTERS.map((key) => [key, null])),
      display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
      display_properties: {},
      rich_filters: {},
    },
    apiBase
  );
}

/** Replace the stored rich_filters expression (reveals the row when active). */
export async function setIssueRichFilters(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  richFilters: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await patchIssueUserProperties(workspaceSlug, projectId, sessionCookie, { rich_filters: richFilters }, apiBase);
}

/** PATCH part of the display record (test setup for layout-dependent panels). */
export async function setIssueDisplayFilters(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  displayFilters: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await patchIssueUserProperties(workspaceSlug, projectId, sessionCookie, { display_filters: displayFilters }, apiBase);
}

export interface ProjectViewSummary {
  id: string;
  name: string;
  rich_filters: unknown;
}

/** Saved project views as the server reports them. */
export async function serverSavedViews(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ProjectViewSummary[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] views read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const view = row as { id?: unknown; name?: unknown; rich_filters?: unknown };
    if (typeof view.id !== "string" || typeof view.name !== "string")
      throw new Error("[parity] view row carried no id/name.");
    return { id: view.id, name: view.name, rich_filters: view.rich_filters };
  });
}

/** Create a saved project view; returns its id (callers delete it after). */
export async function createProjectView(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  richFilters: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const csrf = sessionCookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("csrftoken="))
    ?.slice("csrftoken=".length);
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`, {
    method: "POST",
    headers: {
      cookie: sessionCookie,
      "content-type": "application/json",
      ...(csrf === undefined ? {} : { "X-CSRFToken": csrf }),
    },
    body: JSON.stringify({ name, rich_filters: richFilters }),
  });
  if (!res.ok) throw new Error(`[parity] view create failed with HTTP ${res.status}.`);
  const view = (await res.json()) as { id?: unknown };
  if (typeof view.id !== "string") throw new Error("[parity] view create returned no id.");
  return view.id;
}

/** The project's views/cycle/module feature flags (save UI is flag-gated). */
export async function serverProjectViewFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, boolean>> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const project = (await res.json()) as Record<string, unknown>;
  const pick = (key: string): boolean => project[key] === true;
  return {
    issue_views_view: pick("issue_views_view"),
    cycle_view: pick("cycle_view"),
    module_view: pick("module_view"),
  };
}

/** Flip project feature flags; callers restore what they found. */
export async function setProjectViewFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  flags: Record<string, boolean>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const csrf = sessionCookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("csrftoken="))
    ?.slice("csrftoken=".length);
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "PATCH",
    headers: {
      cookie: sessionCookie,
      "content-type": "application/json",
      ...(csrf === undefined ? {} : { "X-CSRFToken": csrf }),
    },
    body: JSON.stringify(flags),
  });
  if (!res.ok) throw new Error(`[parity] project flags write failed with HTTP ${res.status}.`);
}

/** Delete a saved project view (keeps scenarios from leaking views). */
export async function deleteProjectView(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const csrf = sessionCookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("csrftoken="))
    ?.slice("csrftoken=".length);
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`, {
    method: "DELETE",
    headers: {
      cookie: sessionCookie,
      ...(csrf === undefined ? {} : { "X-CSRFToken": csrf }),
    },
  });
  if (!res.ok) throw new Error(`[parity] view delete failed with HTTP ${res.status}.`);
}

// --- Command palette / search / preferences / shared kit (NEWFRONT-127, SHELL-080-097 + 103-106).
// --- Appended; existing helpers above are untouched per the shared driver contract.

/** Create one project page; resolves with its id. */
export async function serverCreatePage(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) throw new Error(`[parity] page create failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { id?: unknown };
  if (typeof payload.id !== "string") throw new Error("[parity] page create returned no id.");
  return payload.id;
}

/** Whether the cycle currently carries the caller's favorite mark. */
export async function serverCycleIsFavorite(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] cycle read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { is_favorite?: unknown };
  return payload.is_favorite === true;
}

/** Delete one project page (archive-then-delete; throws naming the failure). */
export async function serverDeletePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  // The API refuses to delete a live page (400); archiving first is the
  // same two-step the UI performs.
  const archived = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`,
    { method: "POST", headers: { cookie: sessionCookie } }
  );
  if (!archived.ok) throw new Error(`[parity] page archive failed with HTTP ${archived.status}.`);
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page delete failed with HTTP ${res.status}.`);
}

/**
 * Ensure the workspace user with `email` holds the GUEST project role.
 * Idempotent: re-adding an existing member reactivates the same row, so
 * scenarios heal the seed's intended state instead of depending on it.
 */
export async function serverEnsureProjectGuest(
  workspaceSlug: string,
  projectId: string,
  email: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const membersRes = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!membersRes.ok) throw new Error(`[parity] workspace member list failed with HTTP ${membersRes.status}.`);
  const rows = (await membersRes.json()) as Array<{ member?: { id?: unknown; email?: unknown } }>;
  const userId = rows.find((row) => row.member?.email === email)?.member?.id;
  if (typeof userId !== "string") throw new Error(`[parity] workspace user ${email} not found.`);
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ members: [{ member_id: userId, role: PROJECT_ROLE_GUEST }] }),
  });
  if (!res.ok) throw new Error(`[parity] project guest ensure failed with HTTP ${res.status}.`);
}

/**
 * The browse-route key for the first seeded work item ("IDENT-seq"), which the
 * workspace browse route (`/{slug}/browse/{key}`) resolves to a detail view.
 * SHELL-106 proves that key opens the project-scoped detail, not a browser.
 */
export async function serverFirstWorkItemKey(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ key: string; name: string }> {
  const identifier = await serverProjectIdentifier(workspaceSlug, projectId, sessionCookie, apiBase);
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const first = rows[0] as { sequence_id?: unknown; name?: unknown } | undefined;
  if (!first || typeof first.sequence_id !== "number" || typeof first.name !== "string") {
    throw new Error("[parity] could not resolve the first work item's sequence_id/name.");
  }
  return { key: `${identifier}-${first.sequence_id}`, name: first.name };
}

/** The project's stored cover image reference, or null when unset. */
export async function serverProjectCover(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { cover_image?: unknown };
  return typeof payload.cover_image === "string" ? payload.cover_image : null;
}

/** Set (or clear, with null) the project's cover image reference. */
export async function serverUpdateProjectCover(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  coverImage: string | null,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ cover_image: coverImage }),
  });
  if (!res.ok) throw new Error(`[parity] project cover update failed with HTTP ${res.status}.`);
}

/** The signed-in user's account record (user_timezone, …). */
export async function serverUserAccount(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] user read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

/** The signed-in user's profile (theme, language, start_of_the_week, …). */
export async function serverUserProfile(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetch(`${apiBase}/api/users/me/profile/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

// --- NEWFRONT-118 (layouts B): sign in an existing user into the shared
// --- authenticated-user handle, so kanban/gantt scenarios enter the app
// --- pre-authenticated instead of paying the UI sign-in cost per test.
// --- Appended; existing helpers above are untouched per the shared harness
// --- contract.

/**
 * Sign in an existing account through the native credential endpoint and
 * return the authenticated-user handle (live session cookie plus CSRF
 * token) that `browserCookies` and the authed API helpers consume.
 */
export async function signInFreshUser(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<FreshUser> {
  const cookie = await signInSession(email, password, apiBase);
  const jar = cookieJar(cookie.split(";").map((pair) => pair.trim()));
  const csrfToken = jar.get("csrftoken") ?? "";
  const meRes = await fetchTolerant(`${apiBase}/api/users/me/`, { headers: { cookie } });
  if (!meRes.ok) throw new Error(`[parity] users/me read failed with HTTP ${meRes.status} after sign-in.`);
  const me = (await meRes.json()) as { id?: unknown };
  const userId = typeof me.id === "string" ? me.id : "";
  return { email, password, userId, cookie, csrfToken, apiBase };
}

// --- NEWFRONT-118 (layouts B): kanban/gantt-only API helpers
// --- (cycle-member removal, label deletion). Shared issue/state/
// --- preference fixtures live in the NEWFRONT-117 block below (same
// --- names, adopted, never forked); `cycleId`/`moduleIds` extend the
// --- shared issue details because the kanban cross-column scenarios
// --- assert cycle/module moves through them.

/**
 * Remove one issue from a cycle; throws unless the server accepts.
 * Cycle cleanup must call this before deleting the cycle: deleting a
 * non-empty cycle leaves its members pointing at the deleted id, and
 * those issues then vanish from cycle-grouped boards.
 */
export async function serverRemoveIssueFromCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/${issueId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle-issues remove failed with HTTP ${res.status}.`);
}

/** Delete a project label; throws unless the server accepts. */
export async function serverDeleteLabel(
  workspaceSlug: string,
  projectId: string,
  labelId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issue-labels/${labelId}/`,
    {
      method: "DELETE",
      headers: { cookie: sessionCookie },
    }
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] label delete failed with HTTP ${res.status}.`);
  }
}

// Scratch-project scenarios use the base harness' serverCreateProject
// (workspace, session, name, identifier) and serverDeleteProject;
// deleted-issue cleanup uses the base serverDeleteIssue.

// ---------------------------------------------------------------------------
// Projects list + lifecycle parity helpers (NEWFRONT-124, rows SHELL-024..045).
//
// Added additively to the NEWFRONT-19 harness (never forking the helpers
// above). The projects-list scenarios build their own state on top of the
// public REST API the old app itself calls: the seeded owner drives the
// single-user cases (list, sort, filter, search, favorite, archive, restore,
// delete, create), while the multi-user cases (join, leave, non-member card
// state, guest gating) mint fresh users, seat them in a fresh workspace via
// the native sign-up + invitation-accept flow, and read membership state
// back through the same API. Everything a scenario needs to assert about
// server state — the project collection, favorites, archived_at, membership
// role — is read here so a redesigned screen cannot pass while the server
// stored the wrong thing. Distinct names (…ForProjects / AuthedSession)
// keep this section merge-clean against sibling driver extensions.
// ---------------------------------------------------------------------------

/** Workspace/project role codes the API uses. */
export const ROLE = { ADMIN: 20, MEMBER: 15, GUEST: 5 } as const;

/** Public vs private project `network` codes. */
export const NETWORK = { PRIVATE: 0, PUBLIC: 2 } as const;

/** An authenticated user plus everything needed to act as them over API + browser. */
export interface AuthedSession {
  email: string;
  password: string;
  /** UUID string. */
  userId: string;
  /** `Cookie:` header value for authenticated API calls as this user. */
  cookie: string;
  /** CSRF token matching the `csrftoken` cookie, for unsafe requests. */
  csrfToken: string;
  apiBase: string;
}

/** A cookie shaped for Playwright's `context.addCookies`. */
export interface BrowserSessionCookie {
  name: string;
  value: string;
  domain: string;
  path: string;
  httpOnly: boolean;
  sameSite: "Lax";
}

/** Minimal project shape the scenarios read back from `projects/details/`. */
export interface ProjectListFacts {
  id: string;
  name: string;
  identifier: string;
  network: number;
  is_favorite: boolean;
  archived_at: string | null;
  member_role: number | null;
  members: string[];
}

/** A short suffix unique enough for parallel runs and reruns. */
export function uniqueSuffixForProjects(): string {
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

function jarFromSetCookies(...groups: string[][]): Map<string, string> {
  const jar = new Map<string, string>();
  for (const group of groups) {
    for (const line of group) {
      const first = (line.split(";", 1)[0] ?? "").trim();
      const eq = first.indexOf("=");
      if (eq > 0) jar.set(first.slice(0, eq), first.slice(eq + 1));
    }
  }
  return jar;
}

function jarHeader(jar: Map<string, string>): string {
  return [...jar.entries()].map(([n, v]) => `${n}=${v}`).join("; ");
}

async function fetchCsrfPair(apiBase: string): Promise<{ token: string; setCookies: string[] }> {
  const res = await fetch(`${apiBase}/auth/get-csrf-token/`);
  if (!res.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { csrf_token?: unknown };
  const token = typeof payload.csrf_token === "string" ? payload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  return { token, setCookies: setCookieHeaders(res) };
}

async function loadMe(apiBase: string, cookie: string): Promise<string> {
  const res = await fetch(`${apiBase}/api/users/me/`, { headers: { cookie } });
  if (!res.ok) throw new Error(`[parity] users/me read failed with HTTP ${res.status}.`);
  const me = (await res.json()) as { id?: unknown };
  return typeof me.id === "string" ? me.id : "";
}

/** Sign in an existing user (e.g. the seeded owner) and return their full session. */
export async function signInAuthedSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AuthedSession> {
  const csrf = await fetchCsrfPair(apiBase);
  const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: csrf.token });
  const res = await fetch(`${apiBase}/auth/sign-in/`, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      cookie: jarHeader(jarFromSetCookies(csrf.setCookies)),
      referer: `${apiBase}/`,
    },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302)
    throw new Error(`[parity] sign-in failed with HTTP ${res.status} for ${email}.`);
  const jar = jarFromSetCookies(csrf.setCookies, setCookieHeaders(res));
  const cookie = jarHeader(jar);
  if (!cookie.includes("session-id=")) throw new Error("[parity] sign-in response carried no session cookie.");
  const csrfToken = jar.get("csrftoken") ?? csrf.token;
  return { email, password, userId: await loadMe(apiBase, cookie), cookie, csrfToken, apiBase };
}

/** Create a brand-new account through the native sign-up form; returns its live session. */
export async function signUpAuthedSession(
  prefix = "parity-proj",
  apiBase: string = apiBaseFromEnv()
): Promise<AuthedSession> {
  const email = `${prefix}-${uniqueSuffixForProjects()}@example.com`;
  const password = "Parity-Proj-9x";
  const csrf = await fetchCsrfPair(apiBase);
  const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: csrf.token });
  const res = await fetch(`${apiBase}/auth/sign-up/`, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      cookie: jarHeader(jarFromSetCookies(csrf.setCookies)),
      referer: `${apiBase}/`,
    },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302)
    throw new Error(`[parity] sign-up failed with HTTP ${res.status} for ${email}.`);
  const jar = jarFromSetCookies(csrf.setCookies, setCookieHeaders(res));
  const cookie = jarHeader(jar);
  if (!cookie.includes("session-id=")) throw new Error("[parity] sign-up response carried no session cookie.");
  const csrfToken = jar.get("csrftoken") ?? csrf.token;
  return { email, password, userId: await loadMe(apiBase, cookie), cookie, csrfToken, apiBase };
}

function oracleHost(): string {
  const raw = process.env["PARITY_ORACLE_URL"] ?? "http://localhost:13000";
  try {
    return new URL(raw).hostname;
  } catch {
    return "localhost";
  }
}

/** Cookies for `context.addCookies` so a browser acts as this user with no UI login. */
export function browserSessionCookies(session: AuthedSession): BrowserSessionCookie[] {
  const domain = oracleHost();
  const jar = jarFromSetCookies(session.cookie.split(";").map((p) => p.trim()));
  const out: BrowserSessionCookie[] = [];
  for (const name of ["session-id", "csrftoken"]) {
    const value = jar.get(name);
    if (value !== undefined) out.push({ name, value, domain, path: "/", httpOnly: true, sameSite: "Lax" });
  }
  return out;
}

async function sessionFetch(
  session: AuthedSession,
  method: "GET" | "POST" | "PATCH" | "DELETE",
  path: string,
  jsonBody?: unknown
): Promise<Response> {
  const headers: Record<string, string> = { cookie: session.cookie };
  if (jsonBody !== undefined || method !== "GET") {
    headers["x-csrftoken"] = session.csrfToken;
    headers["referer"] = `${session.apiBase}/`;
  }
  if (jsonBody !== undefined) headers["content-type"] = "application/json";
  // The scratch stack shares one IP throttle bucket with the frontend's own
  // loader calls, so bursts surface as 429/503; back off and retry rather
  // than failing setup on a transient.
  let attempt = 0;
  for (;;) {
    const res = await fetch(`${session.apiBase}${path}`, {
      method,
      headers,
      body: jsonBody === undefined ? undefined : JSON.stringify(jsonBody),
    });
    if ((res.status === 429 || res.status === 503) && attempt < 4) {
      attempt += 1;
      await new Promise((r) => setTimeout(r, 1000 * 2 ** (attempt - 1)));
      continue;
    }
    return res;
  }
}

/** The project collection the list screen reads (`projects/details/`). */
export async function projectsDetails(session: AuthedSession, slug: string): Promise<ProjectListFacts[]> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/projects/details/`);
  if (!res.ok) throw new Error(`[parity] projects/details read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map((r) => ({
    id: String(r["id"]),
    name: String(r["name"]),
    identifier: String(r["identifier"]),
    network: typeof r["network"] === "number" ? (r["network"] as number) : -1,
    is_favorite: r["is_favorite"] === true,
    archived_at: typeof r["archived_at"] === "string" ? (r["archived_at"] as string) : null,
    member_role: typeof r["member_role"] === "number" ? (r["member_role"] as number) : null,
    members: Array.isArray(r["members"]) ? (r["members"] as string[]) : [],
  }));
}

/** One project's facts by name, or undefined when absent from the collection. */
export async function projectByName(
  session: AuthedSession,
  slug: string,
  name: string
): Promise<ProjectListFacts | undefined> {
  return (await projectsDetails(session, slug)).find((p) => p.name === name);
}

/** Create a project via the same POST the create modal submits. Returns its id. */
export async function createProjectViaApi(
  session: AuthedSession,
  slug: string,
  input: { name: string; identifier: string; network?: number }
): Promise<string> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${slug}/projects/`, {
    name: input.name,
    identifier: input.identifier,
    network: input.network ?? NETWORK.PUBLIC,
  });
  if (res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] project create failed with HTTP ${res.status} for ${input.identifier}.`);
  const proj = (await res.json()) as { id?: unknown };
  if (typeof proj.id !== "string") throw new Error("[parity] project create response carried no id.");
  return proj.id;
}

export async function deleteProjectViaApi(session: AuthedSession, slug: string, projectId: string): Promise<void> {
  const res = await sessionFetch(session, "DELETE", `/api/workspaces/${slug}/projects/${projectId}/`);
  if (res.status !== 204 && res.status !== 200)
    throw new Error(`[parity] project delete failed with HTTP ${res.status}.`);
}

export async function archiveProjectViaApi(session: AuthedSession, slug: string, projectId: string): Promise<void> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${slug}/projects/${projectId}/archive/`);
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] project archive failed with HTTP ${res.status}.`);
}

export async function restoreProjectViaApi(session: AuthedSession, slug: string, projectId: string): Promise<void> {
  const res = await sessionFetch(session, "DELETE", `/api/workspaces/${slug}/projects/${projectId}/archive/`);
  if (res.status !== 204 && res.status !== 200)
    throw new Error(`[parity] project restore failed with HTTP ${res.status}.`);
}

export async function addFavoriteViaApi(session: AuthedSession, slug: string, projectId: string): Promise<void> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${slug}/user-favorite-projects/`, {
    project: projectId,
  });
  if (res.status !== 204 && res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] add favorite failed with HTTP ${res.status}.`);
}

/** Create a fresh workspace owned by this user (they become admin, role 20). */
export async function createWorkspaceForProjects(
  session: AuthedSession,
  input: { name: string; slug: string }
): Promise<{ id: string; slug: string }> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/`, {
    name: input.name,
    slug: input.slug,
    organization_size: "2-10",
  });
  if (res.status !== 201 && res.status !== 200)
    throw new Error(`[parity] workspace create failed with HTTP ${res.status} for ${input.slug}.`);
  const ws = (await res.json()) as { id?: unknown; slug?: unknown };
  return { id: String(ws.id), slug: String(ws.slug) };
}

/** Invite emails to a workspace as an admin (bulk invite endpoint). */
export async function inviteToWorkspaceForProjects(
  admin: AuthedSession,
  slug: string,
  invites: Array<{ email: string; role: number }>
): Promise<void> {
  const res = await sessionFetch(admin, "POST", `/api/workspaces/${slug}/invitations/`, { emails: invites });
  if (res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] workspace invite failed with HTTP ${res.status} for ${slug}.`);
}

/** Accept every pending workspace invitation for this user (lands them in the workspace). */
export async function acceptWorkspaceInvitesForProjects(user: AuthedSession, slug: string): Promise<void> {
  const listRes = await sessionFetch(user, "GET", `/api/users/me/workspaces/invitations/`);
  if (!listRes.ok) throw new Error(`[parity] invitations list failed with HTTP ${listRes.status}.`);
  const rows = (await listRes.json()) as Array<{ id?: unknown; workspace?: { slug?: unknown } }>;
  const ids = rows
    .filter((r) => (typeof r.workspace?.slug === "string" ? r.workspace.slug === slug : true))
    .map((r) => (typeof r.id === "string" ? r.id : ""))
    .filter((id) => id.length > 0);
  if (ids.length === 0) throw new Error(`[parity] no pending invitation to ${slug} for ${user.email}.`);
  const res = await sessionFetch(user, "POST", `/api/users/me/workspaces/invitations/`, { invitations: ids });
  if (res.status !== 204 && res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] invitation accept failed with HTTP ${res.status}.`);
}

/** Seat a fresh user in a workspace at the given role, returning their session. */
export async function seatFreshMember(
  admin: AuthedSession,
  slug: string,
  role: number,
  prefix = "parity-member"
): Promise<AuthedSession> {
  const member = await signUpAuthedSession(prefix);
  await inviteToWorkspaceForProjects(admin, slug, [{ email: member.email, role }]);
  await acceptWorkspaceInvitesForProjects(member, slug);
  return member;
}

/** Add workspace users to a project at the given roles (the admin invite endpoint). */
export async function addProjectMembersViaApi(
  admin: AuthedSession,
  slug: string,
  projectId: string,
  members: Array<{ member_id: string; role: number }>
): Promise<void> {
  const res = await sessionFetch(admin, "POST", `/api/workspaces/${slug}/projects/${projectId}/members/`, {
    members,
  });
  if (res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] project member add failed with HTTP ${res.status}.`);
}

/** Join projects via the same self-join POST the join modal submits. */
export async function joinProjectsViaApi(user: AuthedSession, slug: string, projectIds: string[]): Promise<void> {
  const res = await sessionFetch(user, "POST", `/api/users/me/workspaces/${slug}/projects/invitations/`, {
    project_ids: projectIds,
  });
  if (res.status !== 200 && res.status !== 201)
    throw new Error(`[parity] project join failed with HTTP ${res.status}.`);
}

/** This user's role in a project, or null when they are not a member. */
export async function projectMemberRole(user: AuthedSession, slug: string, projectId: string): Promise<number | null> {
  const p = (await projectsDetails(user, slug)).find((x) => x.id === projectId);
  return p ? p.member_role : null;
}

/**
 * Mark a user fully onboarded (profile steps + onboarded marker + tour done)
 * so entering the app lands them on the target page instead of the onboarding
 * funnel or the first-run tour overlay. Call before driving the UI as any
 * freshly minted user.
 */
export async function markOnboardedForProjects(user: AuthedSession): Promise<void> {
  await sessionFetch(user, "PATCH", `/api/users/me/profile/`, {
    onboarding_step: { profile_complete: true, workspace_create: true, workspace_join: true, workspace_invite: true },
  });
  const onboard = await sessionFetch(user, "PATCH", `/api/users/me/onboard/`, { is_onboarded: true });
  if (!onboard.ok) throw new Error(`[parity] onboard marker update failed with HTTP ${onboard.status}.`);
  await sessionFetch(user, "PATCH", `/api/users/me/tour-completed/`, { is_tour_completed: true });
}

/** Point the user's last-visited workspace at an id (so entry lands there). */
export async function setLastWorkspaceForProjects(user: AuthedSession, workspaceId: string): Promise<void> {
  await sessionFetch(user, "PATCH", `/api/users/me/profile/`, { last_workspace_id: workspaceId });
}

/** The feature-view flags a project was created with (work-items-only when all false). */
export interface ProjectFeatureFlags {
  network: number;
  cycle_view: boolean;
  module_view: boolean;
  issue_views_view: boolean;
  page_view: boolean;
  intake_view: boolean;
}

export async function projectFeatureFlags(
  session: AuthedSession,
  slug: string,
  name: string
): Promise<ProjectFeatureFlags | undefined> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/projects/details/`);
  if (!res.ok) throw new Error(`[parity] projects/details read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  const row = rows.find((r) => r["name"] === name);
  if (!row) return undefined;
  const bool = (k: string): boolean => row[k] === true;
  return {
    network: typeof row["network"] === "number" ? (row["network"] as number) : -1,
    cycle_view: bool("cycle_view"),
    module_view: bool("module_view"),
    issue_views_view: bool("issue_views_view"),
    page_view: bool("page_view"),
    intake_view: bool("intake_view"),
  };
}

/** Set a project's lead to a workspace member (the same PATCH the UI sends). */
export async function setProjectLeadViaApi(
  session: AuthedSession,
  slug: string,
  projectId: string,
  userId: string
): Promise<void> {
  const res = await sessionFetch(session, "PATCH", `/api/workspaces/${slug}/projects/${projectId}/`, {
    project_lead: userId,
  });
  if (!res.ok) throw new Error(`[parity] project lead set failed with HTTP ${res.status}.`);
}

/** A project's stored timezone, or null when the field is missing/empty. */
export async function projectTimezone(session: AuthedSession, slug: string, name: string): Promise<string | null> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/projects/details/`);
  if (!res.ok) throw new Error(`[parity] projects/details read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  const row = rows.find((r) => r["name"] === name);
  const tz = row?.["timezone"];
  return typeof tz === "string" && tz.length > 0 ? tz : null;
}

// Issues-layouts parity helpers (NEWFRONT-117, rows ISS-001..027, ISS-060..074).
//
// Added additively to the NEWFRONT-19 harness (never altering the helpers
// above). Layout scenarios drive the UI for what the user sees and read
// back through these helpers for what the server stored: issue field
// values after inline edits, per-user layout preferences after switching,
// and fixture builders (states, dated issues, cycles, modules, labels,
// archived rows) with cleanup so each scenario leaves the seeded project
// exactly as the seed left it.
// ---------------------------------------------------------------------------

/** Subset of an issue row the layout scenarios assert on. */
export interface LayoutsIssueDetails {
  id: string;
  name: string;
  stateId: string;
  priority: string | null;
  targetDate: string | null;
  startDate: string | null;
  assigneeIds: string[];
  labelIds: string[];
  parentId: string | null;
  sequenceId: number;
  sortOrder: number;
  // NEWFRONT-118: kanban cross-column scenarios assert cycle/module moves
  // through these; layouts-A readers ignore them.
  cycleId: string | null;
  moduleIds: string[];
}

/** One issue row as the server reports it. */
export async function serverIssueDetails(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LayoutsIssueDetails> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    {
      headers: { cookie: sessionCookie },
    }
  );
  if (!res.ok) throw new Error(`[parity] issue read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as {
    id?: unknown;
    name?: unknown;
    state_id?: unknown;
    state?: unknown;
    priority?: unknown;
    target_date?: unknown;
    start_date?: unknown;
    assignee_ids?: unknown;
    label_ids?: unknown;
    parent_id?: unknown;
    parent?: unknown;
    sequence_id?: unknown;
    sort_order?: unknown;
    cycle_id?: unknown;
    cycle?: unknown;
    module_ids?: unknown;
  };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] issue row carried no string id and name.");
  }
  const stateId = record.state_id ?? record.state;
  if (typeof stateId !== "string") throw new Error("[parity] issue row carried no state id.");
  const sequenceId = record.sequence_id;
  if (typeof sequenceId !== "number") throw new Error("[parity] issue row carried no numeric sequence_id.");
  const sortOrder = record.sort_order;
  if (typeof sortOrder !== "number") throw new Error("[parity] issue row carried no numeric sort_order.");
  const cycleId = record.cycle_id ?? record.cycle;
  return {
    id: record.id,
    name: record.name,
    stateId,
    priority: typeof record.priority === "string" ? record.priority : null,
    targetDate: typeof record.target_date === "string" ? record.target_date : null,
    startDate: typeof record.start_date === "string" ? record.start_date : null,
    assigneeIds: Array.isArray(record.assignee_ids)
      ? record.assignee_ids.filter((v): v is string => typeof v === "string")
      : [],
    labelIds: Array.isArray(record.label_ids) ? record.label_ids.filter((v): v is string => typeof v === "string") : [],
    parentId:
      typeof record.parent_id === "string"
        ? record.parent_id
        : typeof record.parent === "string"
          ? record.parent
          : null,
    sequenceId,
    sortOrder,
    cycleId: typeof cycleId === "string" ? cycleId : null,
    moduleIds: Array.isArray(record.module_ids)
      ? record.module_ids.filter((v): v is string => typeof v === "string")
      : [],
  };
}

/** A project state row the layout scenarios assert or build on. */
export interface LayoutsState {
  id: string;
  name: string;
  group: string;
  isDefault: boolean;
}

/** Every state of a project, in API order. */
export async function serverListStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LayoutsState[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] states read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown; group?: unknown; default?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string") {
      throw new Error("[parity] state row carried no string id, name and group.");
    }
    return { id: record.id, name: record.name, group: record.group, isDefault: record.default === true };
  });
}

/** Per-user layout preferences the server stores for one entity. */
export interface LayoutsUserProperties {
  displayFilters: Record<string, unknown>;
  displayProperties: Record<string, unknown>;
  richFilters?: unknown;
}

/** Read the caller's project-level layout preferences. */
export async function serverProjectUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LayoutsUserProperties> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] user-properties read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as {
    display_filters?: unknown;
    display_properties?: unknown;
    rich_filters?: unknown;
  };
  const displayFilters = record.display_filters;
  const displayProperties = record.display_properties;
  if (typeof displayFilters !== "object" || displayFilters === null) {
    throw new Error("[parity] user-properties carried no display_filters object.");
  }
  if (typeof displayProperties !== "object" || displayProperties === null) {
    throw new Error("[parity] user-properties carried no display_properties object.");
  }
  return {
    displayFilters: displayFilters as Record<string, unknown>,
    displayProperties: displayProperties as Record<string, unknown>,
    richFilters: record.rich_filters,
  };
}

/**
 * Write the caller's project-level layout preferences (scenario setup and
 * teardown). Pass the full objects: the endpoint merges them over the
 * stored row. `filters` carries the issue filters (empty-state scenarios
 * match nothing through it); the seed default is all nulls.
 */
export async function serverPatchProjectUserProperties(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  patch: {
    filters?: Record<string, unknown>;
    display_filters?: Record<string, unknown>;
    display_properties?: Record<string, unknown>;
    rich_filters?: unknown;
  },
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-properties/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`[parity] user-properties patch failed with HTTP ${res.status}.`);
}

/** Seed-equivalent project preferences; scenarios restore these when done. */
export function seedProjectUserProperties(): {
  display_filters: Record<string, unknown>;
  display_properties: Record<string, unknown>;
} {
  return {
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
  };
}

/** Restore one archived issue; throws unless the server accepts. */
export async function serverRestoreIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] issue restore failed with HTTP ${res.status}.`);
}

/** Archived issues of a project, in API order. */
export async function serverArchivedIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string") {
      throw new Error("[parity] archived row carried no string id and name.");
    }
    return { id: record.id, name: record.name };
  });
}

/** Move one issue to another project; throws unless the server accepts. */
export async function serverMoveIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  targetProjectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/work-items/${issueId}/move/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ project: targetProjectId }),
    }
  );
  if (!res.ok) throw new Error(`[parity] issue move failed with HTTP ${res.status}.`);
}

/** Attach issues to a cycle; throws unless the server accepts. */
export async function serverAddIssuesToCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  issueIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ issues: issueIds }),
    }
  );
  if (!res.ok) throw new Error(`[parity] cycle-issues add failed with HTTP ${res.status}.`);
}

/**
 * Browser cookies from a `signInSession` header, for `openAuthenticated`.
 * Layout specs authenticate this way instead of driving the sign-in card:
 * the card belongs to the auth rows, and skipping it keeps each scenario
 * focused on its own area while staying immune to card-load flakiness.
 */
export function sessionBrowserCookies(sessionCookie: string): BrowserCookie[] {
  const jar = cookieJar(sessionCookie.split(";").map((pair) => pair.trim()));
  const sessionId = jar.get("session-id");
  if (sessionId === undefined) throw new Error("[parity] session cookie carried no session-id.");
  const domain = oracleHostFromEnv();
  const cookies: BrowserCookie[] = [
    { name: "session-id", value: sessionId, domain, path: "/", httpOnly: true, sameSite: "Lax" },
  ];
  const csrf = jar.get("csrftoken");
  if (csrf !== undefined) {
    cookies.push({ name: "csrftoken", value: csrf, domain, path: "/", httpOnly: true, sameSite: "Lax" });
  }
  return cookies;
}

/** Resolve a workspace member's user id by email; throws when absent. */
export async function serverWorkspaceUserId(
  workspaceSlug: string,
  email: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  // The members read occasionally answers a partial page under host
  // contention, so a miss retries instead of failing the scenario.
  let lastCount = 0;
  for (let attempt = 0; attempt < 3; attempt++) {
    const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/members/`, {
      headers: { cookie: sessionCookie },
    });
    if (!res.ok) throw new Error(`[parity] workspace members read failed with HTTP ${res.status}.`);
    const payload: unknown = await res.json();
    const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
    lastCount = rows.length;
    for (const row of rows) {
      // Rows nest the user record: { id: <membership>, member: { id: <user>, email } }.
      const record = row as { member?: unknown };
      const member =
        typeof record.member === "object" && record.member !== null
          ? (record.member as { id?: unknown; email?: unknown })
          : null;
      if (member?.email === email && typeof member.id === "string") return member.id;
    }
    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
  throw new Error(`[parity] no workspace member carries email ${email} (saw ${lastCount} rows).`);
}

/** Attach issues to a module; throws unless the server accepts. */
export async function serverAddIssuesToModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  issueIds: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  // Add issues through .../modules/{id}/issues/ (the module service's
  // route); .../module-issues/ is the list/detail route and 404s a POST.
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/issues/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ issues: issueIds }),
    }
  );
  if (!res.ok) throw new Error(`[parity] module-issues add failed with HTTP ${res.status}.`);
}

/** Project feature flags the layout scenarios assert or toggle (ISS-016). */
export interface LayoutsProjectDetails {
  id: string;
  name: string;
  cycleView: boolean;
  moduleView: boolean;
  identifier: string;
}

/** One project's detail row: flags the column specs toggle and restore. */
export async function serverProjectDetails(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<LayoutsProjectDetails> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as {
    id?: unknown;
    name?: unknown;
    cycle_view?: unknown;
    module_view?: unknown;
    identifier?: unknown;
  };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] project row missed id or name.");
  }
  return {
    id: record.id,
    name: record.name,
    cycleView: record.cycle_view === true,
    moduleView: record.module_view === true,
    identifier: typeof record.identifier === "string" ? record.identifier : "",
  };
}

/** Issue ids currently attached to a cycle (bridge list, shape-tolerant). */
export async function serverCycleIssueIds(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const ids: string[] = [];
  for (const row of rows) {
    const r = row as { id?: unknown; issue?: unknown };
    if (typeof r.id === "string") ids.push(r.id);
    else if (typeof r.issue === "string") ids.push(r.issue);
    else if (r.issue !== null && typeof r.issue === "object" && typeof (r.issue as { id?: unknown }).id === "string") {
      ids.push((r.issue as { id: string }).id);
    }
  }
  return ids;
}

/** Issue ids currently attached to a module (shape-tolerant). */
export async function serverModuleIssueIds(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  // The /api/ tree serves the module's issues at .../issues/ (GET);
  // module-issues/ lives only under /api/v1/ and 404s here.
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/issues/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] module-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const ids: string[] = [];
  for (const row of rows) {
    const r = row as { id?: unknown; issue?: unknown };
    if (typeof r.id === "string") ids.push(r.id);
    else if (typeof r.issue === "string") ids.push(r.issue);
    else if (r.issue !== null && typeof r.issue === "object" && typeof (r.issue as { id?: unknown }).id === "string") {
      ids.push((r.issue as { id: string }).id);
    }
  }
  return ids;
}

/** Delete a saved project view; throws unless the server accepts. */
export async function serverDeleteView(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] view delete failed with HTTP ${res.status}.`);
  }
}

/** Seed-equivalent issue filters (all nulls); empty-state scenarios restore these. */
export function seedIssueFilters(): Record<string, unknown> {
  return {
    priority: null,
    state: null,
    state_group: null,
    assignees: null,
    created_by: null,
    labels: null,
    start_date: null,
    target_date: null,
    subscriber: null,
  };
}

/** Write the caller's cycle-level preferences (filters/display shape as project). */
export async function serverPatchCycleUserProperties(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  patch: {
    filters?: Record<string, unknown>;
    display_filters?: Record<string, unknown>;
    display_properties?: Record<string, unknown>;
    rich_filters?: unknown;
  },
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/user-properties/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(patch),
    }
  );
  if (!res.ok) throw new Error(`[parity] cycle user-properties patch failed with HTTP ${res.status}.`);
}

/** Write the caller's module-level preferences (filters/display shape as project). */
export async function serverPatchModuleUserProperties(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  patch: {
    filters?: Record<string, unknown>;
    display_filters?: Record<string, unknown>;
    display_properties?: Record<string, unknown>;
    rich_filters?: unknown;
  },
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/user-properties/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(patch),
    }
  );
  if (!res.ok) throw new Error(`[parity] module user-properties patch failed with HTTP ${res.status}.`);
}

/**
 * The signed-in user's week-start day (0 = Sunday) from the profile the
 * timeline reads. Gantt scenarios cross-check the rendered week rows
 * against it.
 */
export async function serverProfileStartOfWeek(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchTolerant(`${apiBase}/api/users/me/profile/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { start_of_the_week?: unknown };
  if (typeof record.start_of_the_week !== "number") {
    throw new Error("[parity] profile carried no numeric start_of_the_week.");
  }
  return record.start_of_the_week;
}

// --- Desktop-only chat + agent runtime fixtures and server reads
// --- (NEWFRONT-182, RUN-033–036, RUN-044–045, RUN-047). Appended;
// --- existing helpers above are untouched per the shared harness
// --- contract. The desktop rows are proven by their web-observable side:
// --- absent UI plus the server gates a web session hits (the managed
// --- endpoints refuse non-desktop sessions), so this block plants the
// --- cloud fixtures the oracle needs (runners, chat sessions, chat
// --- approvals) and probes the desktop-gated endpoints raw.

/** Runner statuses the desktop-contact scenarios plant (RUN-011 set, reused here). */
export type DesktopRuntimeRunnerStatus = "online" | "busy" | "offline" | "revoked";

/** Minimal runner shape the desktop-runtime scenarios assert on. */
export interface DesktopRuntimeRunner {
  id: string;
  name: string;
  status: string;
  podId: string;
}

/** Minimal chat-session shape the desktop-runtime scenarios assert on. */
export interface DesktopRuntimeChatSession {
  id: string;
  status: string;
  runner: string;
}

/** Minimal chat-message shape the desktop-runtime scenarios assert on. */
export interface DesktopRuntimeChatMessage {
  id: string;
  role: string;
  content: string;
  seq: number;
}

/** Minimal chat-approval shape the desktop-runtime scenarios assert on. */
export interface DesktopRuntimeChatApproval {
  id: string;
  session: string;
  kind: string;
  status: string;
}

/** Raw refusal of a desktop-gated endpoint: status plus its machine-readable error, if any. */
export interface DesktopRuntimeEndpointRefusal {
  status: number;
  error: string;
}

/** API container for shell snippets; the base helper pins parity19-api. */
function desktopRuntimeApiContainer(): string {
  const override = process.env["PARITY_API_CONTAINER"];
  return override !== undefined && override.trim() !== "" ? override.trim() : "parity19-api";
}

/** Run a Django shell snippet inside this run's stack api container. */
export async function desktopRuntimeShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", desktopRuntimeApiContainer(), "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

function desktopRuntimeRunnerOf(raw: unknown): DesktopRuntimeRunner {
  const record = raw as { id?: unknown; name?: unknown; status?: unknown; pod?: unknown; pod_detail?: unknown };
  const podDetail = record.pod_detail as { id?: unknown } | undefined;
  const podId = typeof record.pod === "string" ? record.pod : typeof podDetail?.id === "string" ? podDetail.id : "";
  if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.status !== "string") {
    throw new Error("[parity] runner payload carried no id/name/status.");
  }
  return { id: record.id, name: record.name, status: record.status, podId };
}

function desktopRuntimeChatSessionOf(raw: unknown): DesktopRuntimeChatSession {
  const record = raw as { id?: unknown; status?: unknown; runner?: unknown };
  if (typeof record.id !== "string" || typeof record.status !== "string" || typeof record.runner !== "string") {
    throw new Error("[parity] chat-session payload carried no id/status/runner.");
  }
  return { id: record.id, status: record.status, runner: record.runner };
}

function desktopRuntimeChatMessageOf(raw: unknown): DesktopRuntimeChatMessage {
  const record = raw as { id?: unknown; role?: unknown; content?: unknown; seq?: unknown };
  if (
    typeof record.id !== "string" ||
    typeof record.role !== "string" ||
    typeof record.content !== "string" ||
    typeof record.seq !== "number"
  ) {
    throw new Error("[parity] chat-message payload missed id/role/content/seq.");
  }
  return { id: record.id, role: record.role, content: record.content, seq: record.seq };
}

function desktopRuntimeChatApprovalOf(raw: unknown): DesktopRuntimeChatApproval {
  const record = raw as { id?: unknown; session?: unknown; kind?: unknown; status?: unknown };
  if (
    typeof record.id !== "string" ||
    typeof record.session !== "string" ||
    typeof record.kind !== "string" ||
    typeof record.status !== "string"
  ) {
    throw new Error("[parity] chat-approval payload missed id/session/kind/status.");
  }
  return { id: record.id, session: record.session, kind: record.kind, status: record.status };
}

/** Workspace UUID for a slug (runner rows key off the id, not the slug). */
export async function serverDesktopRuntimeWorkspaceId(workspaceSlug: string): Promise<string> {
  const out = await desktopRuntimeShell(
    `from pi_dash.db.models import Workspace\n` +
      `ws = Workspace.objects.filter(slug=${JSON.stringify(workspaceSlug)}).first()\n` +
      `print("PARITY_WS:" + (str(ws.id) if ws else ""))\n`
  );
  const id = /^PARITY_WS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error(`[parity] no workspace for slug ${workspaceSlug}.`);
  return id;
}

/**
 * Plant a runner row owned by `ownerEmail` in the seeded project, at the
 * given status. Names must be unique per run (callers add a timestamp);
 * there is no web create endpoint, so this goes through the shell.
 * Online/busy runners also get a live RunnerSession row: the outbox
 * treats a runner with no active session as offline, so a planted
 * "online" runner needs the session row to behave like a connected
 * daemon. Offline/revoked runners get none.
 */
export async function serverDesktopRuntimePlantRunner(input: {
  ownerEmail: string;
  workspaceSlug: string;
  projectId: string;
  name: string;
  status: DesktopRuntimeRunnerStatus;
}): Promise<DesktopRuntimeRunner> {
  const out = await desktopRuntimeShell(
    `import json\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import Pod, Runner, RunnerSession\n` +
      `user = User.objects.get(email=${JSON.stringify(input.ownerEmail)})\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `pod = Pod.default_for_project_id(${JSON.stringify(input.projectId)})\n` +
      `runner = Runner.objects.create(owner=user, workspace=ws, pod=pod, name=${JSON.stringify(input.name)}, status=${JSON.stringify(input.status)})\n` +
      `live = ${JSON.stringify(input.status)} in ("online", "busy")\n` +
      `if live:\n` +
      `    RunnerSession.objects.create(runner=runner, last_seen_at=timezone.now())\n` +
      `print("PARITY_RUNNER:" + json.dumps({"id": str(runner.id), "name": runner.name, "status": runner.status, "pod": str(runner.pod_id)}))\n`
  );
  const line = /^PARITY_RUNNER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] runner plant produced no row for ${input.name}.`);
  return desktopRuntimeRunnerOf(JSON.parse(line) as unknown);
}

/** Best-effort runner cleanup; cascades to sessions, messages and approvals. */
export async function serverDesktopRuntimeCleanupRunner(runnerId: string): Promise<void> {
  try {
    await desktopRuntimeShell(
      `from pi_dash.runner.models import Runner\n` +
        `Runner.objects.filter(pk=${JSON.stringify(runnerId)}).delete()\n` +
        `print("PARITY_RUNNER_CLEANUP_OK")\n`
    );
  } catch (error) {
    console.log(`[parity] runner cleanup failed for ${runnerId}; leaving it for reseed. ${String(error)}`);
  }
}

/**
 * Create a chat session through the app's own endpoint. The endpoint
 * reuses an open message-less session with HTTP 200 instead of creating
 * a duplicate, so both 200 (reused) and 201 (created) resolve.
 */
export async function serverDesktopRuntimeCreateChatSession(
  input: { workspaceId: string; runnerId: string },
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeChatSession> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/`, sessionCookie, {
    workspace: input.workspaceId,
    runner: input.runnerId,
  });
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] chat-session create failed with HTTP ${res.status}.`);
  }
  return desktopRuntimeChatSessionOf((await res.json()) as unknown);
}

/** Chat sessions through the same endpoint the history panel reads. */
export async function serverDesktopRuntimeListChatSessions(
  workspaceId: string,
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeChatSession[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/runners/chat/sessions/?workspace=${encodeURIComponent(workspaceId)}&runner=${encodeURIComponent(runnerId)}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] chat-session list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(desktopRuntimeChatSessionOf);
}

/** Messages of a session through the app's own endpoint. */
export async function serverDesktopRuntimeChatMessages(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeChatMessage[]> {
  const res = await fetchTolerant(`${apiBase}/api/runners/chat/sessions/${sessionId}/messages/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] chat-message list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(desktopRuntimeChatMessageOf);
}

/**
 * Send a chat message through the app's own endpoint. Resolves with the
 * stored user message; the session is left mid-turn, exactly as after a
 * UI send with no daemon to answer it.
 */
export async function serverDesktopRuntimeSendChatMessage(
  sessionId: string,
  content: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeChatMessage> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/messages/`, sessionCookie, {
    content,
    content_parts: [],
  });
  if (res.status !== 201) throw new Error(`[parity] chat send failed with HTTP ${res.status}.`);
  return desktopRuntimeChatMessageOf((await res.json()) as unknown);
}

/**
 * Plant a pending chat-approval request on a session through the shell,
 * standing in for the daemon write-back. Returns the approval id.
 */
export async function serverDesktopRuntimePlantChatApproval(input: {
  sessionId: string;
  localApprovalId: string;
  kind: string;
  reason: string;
  payload: Record<string, unknown>;
}): Promise<string> {
  const out = await desktopRuntimeShell(
    `import json\n` +
      `from pi_dash.runner.models import AgentChatSession, AgentChatApprovalRequest\n` +
      `session = AgentChatSession.objects.get(pk=${JSON.stringify(input.sessionId)})\n` +
      `approval = AgentChatApprovalRequest.objects.create(session=session, local_approval_id=${JSON.stringify(input.localApprovalId)}, kind=${JSON.stringify(input.kind)}, reason=${JSON.stringify(input.reason)}, payload=json.loads(${JSON.stringify(JSON.stringify(input.payload))}))\n` +
      `print("PARITY_CHAT_APPROVAL:" + str(approval.id))\n`
  );
  const id = /^PARITY_CHAT_APPROVAL:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error("[parity] chat-approval plant produced no row.");
  return id;
}

/** Pending chat approvals through the user-facing queue endpoint. */
export async function serverDesktopRuntimeListChatApprovals(
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeChatApproval[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/runners/chat/approvals/?workspace=${encodeURIComponent(workspaceId)}`,
    {
      headers: { cookie: sessionCookie },
    }
  );
  if (!res.ok) throw new Error(`[parity] chat-approval list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(desktopRuntimeChatApprovalOf);
}

/**
 * Collapse a sign-in cookie header to one value per cookie the way a
 * browser jar holds it (last wins). Sign-in rotates the CSRF token, so
 * the minted header carries the pre-login token ahead of the live one;
 * the strict v1 endpoints reject that stale pair before the desktop gate
 * runs, which would pin a harness artifact instead of the row behavior.
 */
function desktopRuntimeFreshCookieJar(sessionCookie: string): string {
  const jar = new Map<string, string>();
  for (const pair of sessionCookie.split(";")) {
    const trimmed = pair.trim();
    if (trimmed === "") continue;
    const cut = trimmed.indexOf("=");
    jar.set(cut === -1 ? trimmed : trimmed.slice(0, cut), trimmed);
  }
  return [...jar.values()].join("; ");
}

async function desktopRuntimeRefusalOf(res: Response): Promise<DesktopRuntimeEndpointRefusal> {
  let error = "";
  try {
    const payload = (await res.json()) as { error?: unknown };
    if (typeof payload.error === "string") error = payload.error;
  } catch {
    error = "";
  }
  return { status: res.status, error };
}

/** Raw response of the agent-profile read for a web session (the desktop gate). */
export async function serverDesktopRuntimeAgentProfileRefusal(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeEndpointRefusal> {
  const res = await fetchTolerant(`${apiBase}/api/users/me/ai-assistant/agent-profile/`, {
    headers: { cookie: sessionCookie },
  });
  return desktopRuntimeRefusalOf(res);
}

/** Raw response of the agent-token issue for a web session (the desktop gate). */
export async function serverDesktopRuntimeAgentTokenRefusal(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeEndpointRefusal> {
  const res = await mutateJSON("POST", `${apiBase}/api/users/me/ai-assistant/agent-token/`, sessionCookie, {});
  return desktopRuntimeRefusalOf(res);
}

/** Raw response of the desktop-enroll endpoint for a web session (the desktop gate). */
export async function serverDesktopRuntimeDesktopEnrollRefusal(
  method: "POST" | "DELETE",
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DesktopRuntimeEndpointRefusal> {
  const res = await mutateJSON(
    method,
    `${apiBase}/api/v1/runner/dev-machines/desktop-enroll/`,
    desktopRuntimeFreshCookieJar(sessionCookie),
    {}
  );
  return desktopRuntimeRefusalOf(res);
}

// --- Scheduler catalog + definitions (NEWFRONT-184, AGT-001..006, AGT-022).
// --- Appended; existing helpers above are untouched per the shared harness
// --- contract. Session-cookie style: every helper takes the workspace slug,
// --- a signed-in session cookie, and the ids it needs. Throwing verbs assert
// --- the happy-path status; *Status siblings resolve with the outcome so
// --- refusal halves (403/404/400) can assert instead of throwing.

/** A workspace scheduler definition as the list/detail endpoints return it. */
export interface ParityScheduler {
  id: string;
  slug: string;
  name: string;
  description: string;
  prompt: string;
  color: string;
  source: "builtin" | "manifest";
  is_enabled: boolean;
  active_binding_count: number;
  created_at: string;
  updated_at: string;
}

/** Definition create payload; mirrors the dialog fields. */
export interface ParitySchedulerInput {
  slug: string;
  name: string;
  description?: string;
  prompt: string;
  color?: string;
  is_enabled?: boolean;
}

function schedulerRows(payload: unknown): ParityScheduler[] {
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows as ParityScheduler[];
}

/** List every definition in a workspace (any workspace role may read). */
export async function serverSchedulers(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityScheduler[]> {
  const res = await apiJson("GET", `/api/workspaces/${workspaceSlug}/schedulers/`, sessionCookie, undefined, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] scheduler list failed with HTTP ${res.status}.`);
  }
  return schedulerRows(res.payload);
}

/** Retrieve one definition (workspace admin only). */
export async function serverScheduler(
  workspaceSlug: string,
  schedulerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityScheduler> {
  const res = await serverSchedulerStatus(workspaceSlug, schedulerId, sessionCookie, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] scheduler retrieve failed with HTTP ${res.status}.`);
  }
  return res.payload as ParityScheduler;
}

/** Retrieve one definition; resolves with the outcome instead of throwing on 4xx. */
export async function serverSchedulerStatus(
  workspaceSlug: string,
  schedulerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
}

/** Create a definition (workspace admin only); resolves with the stored row. */
export async function serverCreateScheduler(
  workspaceSlug: string,
  sessionCookie: string,
  input: ParitySchedulerInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityScheduler> {
  const res = await serverCreateSchedulerStatus(workspaceSlug, sessionCookie, input, apiBase);
  if (res.status !== 201) {
    throw new Error(`[parity] scheduler create failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParityScheduler;
}

/** Create a definition; resolves with the outcome instead of throwing on 4xx. */
export async function serverCreateSchedulerStatus(
  workspaceSlug: string,
  sessionCookie: string,
  input: ParitySchedulerInput,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson("POST", `/api/workspaces/${workspaceSlug}/schedulers/`, sessionCookie, input, apiBase);
}

/** Patch a definition (workspace admin only); the handle stays out of the patch. */
export async function serverPatchScheduler(
  workspaceSlug: string,
  schedulerId: string,
  sessionCookie: string,
  patch: Partial<Pick<ParityScheduler, "name" | "description" | "prompt" | "color" | "is_enabled">>,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityScheduler> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`,
    sessionCookie,
    patch,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] scheduler patch failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParityScheduler;
}

/** Soft-delete a definition (workspace admin only); its bindings stop firing. */
export async function serverDeleteScheduler(
  workspaceSlug: string,
  schedulerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/schedulers/${schedulerId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] scheduler delete failed with HTTP ${res.status}.`);
  }
}

/**
 * Convergent definition builder: returns the live row when `input.slug`
 * already exists (a retry reuses it), otherwise creates it. A create that
 * loses a slug race re-lists instead of throwing.
 */
export async function ensureScheduler(
  workspaceSlug: string,
  sessionCookie: string,
  input: ParitySchedulerInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityScheduler> {
  const existing = (await serverSchedulers(workspaceSlug, sessionCookie, apiBase)).find(
    (row) => row.slug === input.slug
  );
  if (existing !== undefined) return existing;
  const created = await serverCreateSchedulerStatus(workspaceSlug, sessionCookie, input, apiBase);
  if (created.status === 201) return created.payload as ParityScheduler;
  const raced = (await serverSchedulers(workspaceSlug, sessionCookie, apiBase)).find((row) => row.slug === input.slug);
  if (raced !== undefined) return raced;
  throw new Error(`[parity] scheduler create failed with HTTP ${created.status}: ${JSON.stringify(created.payload)}`);
}

/** Best-effort definition delete for teardown; never throws. */
export async function serverCleanupScheduler(
  workspaceSlug: string,
  schedulerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  try {
    await serverDeleteScheduler(workspaceSlug, schedulerId, sessionCookie, apiBase);
  } catch {
    // Teardown only: the row is already gone or the stack is draining.
  }
}

/** One scheduler install on a project, as the bindings list returns it. */
export interface ParitySchedulerBinding {
  id: string;
  scheduler: string;
  scheduler_slug: string;
  scheduler_name: string;
  scheduler_color: string;
  project: string;
  dtstart: string;
  tzid: string;
  rrule: string;
  rdates: string[];
  exdates: string[];
  extra_context: string;
  enabled: boolean;
  outcome_mode: string;
  pod: string | null;
  pod_name: string | null;
  next_run_at: string | null;
  last_run: string | null;
  last_run_status: string | null;
  last_run_ended_at: string | null;
  last_error: string;
  actor: string | null;
  created_at: string;
  updated_at: string;
}

/** Binding detail adds the composed run prompt plus the parent's live flags. */
export interface ParitySchedulerBindingDetail extends ParitySchedulerBinding {
  resolved_prompt: string;
  run_count: number;
  scheduler_source: "builtin" | "manifest";
  scheduler_is_enabled: boolean;
}

/** Binding install payload; `project` repeats the URL id (the API requires it). */
export interface ParitySchedulerBindingInput {
  scheduler: string;
  project: string;
  dtstart: string;
  tzid?: string;
  rrule: string;
  rdates?: string[];
  exdates?: string[];
  extra_context?: string;
  enabled?: boolean;
  outcome_mode?: string;
  /** Pod override id, or null/omitted for the project default. */
  pod?: string | null;
}

function bindingRows(payload: unknown): ParitySchedulerBinding[] {
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows as ParitySchedulerBinding[];
}

/** List a project's installs (any project role may read). */
export async function serverBindings(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerBinding[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] binding list failed with HTTP ${res.status}.`);
  }
  return bindingRows(res.payload);
}

/** Read one install's detail payload (any project role may read). */
export async function serverBindingDetail(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerBindingDetail> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] binding detail failed with HTTP ${res.status}.`);
  }
  return res.payload as ParitySchedulerBindingDetail;
}

/** Install a definition on a project (project admin only). */
export async function serverCreateBinding(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  input: ParitySchedulerBindingInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerBinding> {
  const res = await serverCreateBindingStatus(workspaceSlug, projectId, sessionCookie, input, apiBase);
  if (res.status !== 201) {
    throw new Error(`[parity] binding create failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParitySchedulerBinding;
}

/** Install a definition; resolves with the outcome instead of throwing on 4xx. */
export async function serverCreateBindingStatus(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  input: ParitySchedulerBindingInput,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/`,
    sessionCookie,
    input,
    apiBase
  );
}

/** Uninstall one binding (project admin only). */
export async function serverDeleteBinding(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] binding delete failed with HTTP ${res.status}.`);
  }
}

/** Best-effort binding delete for teardown; never throws. */
export async function serverCleanupBinding(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  try {
    await serverDeleteBinding(workspaceSlug, projectId, bindingId, sessionCookie, apiBase);
  } catch {
    // Teardown only: the row is already gone or the stack is draining.
  }
}

// --- Dev machines, runner detail, activity fixtures and server reads
// --- (NEWFRONT-183, RUN-037–043). Appended; existing helpers above are
// --- untouched per the shared harness contract. The web API offers no
// --- create-machine endpoint, so machines, tokens, runners and live
// --- states are planted through the Django shell; the reads below go
// --- through the app's own REST endpoints (the same ones the UI polls)
// --- plus shell probes for token/session revocation the API never
// --- exposes. The scratch stack runs no live runner daemon.

/** Runner statuses the dev-machines scenarios plant. */
export type DevMachinesRunnerStatus = "online" | "busy" | "offline" | "revoked";

/** Minimal planted-machine shape the dev-machines scenarios assert on. */
export interface DevMachinesMachine {
  id: string;
  label: string;
  hostLabel: string;
  revokedAt: string | null;
}

/** One dev-machines list row as the list endpoint returns it. */
export interface DevMachinesMachineRow {
  id: string;
  label: string;
  hostLabel: string;
  revokedAt: string | null;
  runnerCount: number;
  onlineRunnerCount: number;
  lastSeenAt: string | null;
  lastHeartbeatAt: string | null;
}

/** Minimal runner shape the dev-machines scenarios assert on. */
export interface DevMachinesRunner {
  id: string;
  name: string;
  status: string;
  podId: string;
}

// --- Runner chat fixtures + server reads (NEWFRONT-181, RUN-025–032). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. The web API offers no create-runner endpoint (runners arrive
// through device enrollment), so Runner/Pod rows are planted through the
// Django shell while chat sessions and messages go through the same REST
// endpoints the old app calls.

/** API container for shell snippets; parallel stacks override the stock name. */
function runnerChatApiContainer(): string {
  const override = process.env["PARITY_API_CONTAINER"];
  return override !== undefined && override.trim() !== "" ? override.trim() : "parity19-api";
}

/** Run a Django shell snippet inside this run's stack api container. */
export async function runnerChatShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", runnerChatApiContainer(), "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/** Runner statuses the chat page branches on (RUN-011 set, reused here). */
export type RunnerChatStatus = "online" | "busy" | "offline" | "revoked";

/** Minimal runner shape the chat scenarios assert on. */
export interface RunnerChatRunner {
  id: string;
  name: string;
  status: string;
  podId: string;
}

/** Observability snapshot as the runner-detail endpoint returns it. */
export interface DevMachinesLiveState {
  lastEventAt: string | null;
  lastEventKind: string | null;
  lastEventSummary: string | null;
  agentPid: number | null;
  subprocessAlive: boolean | null;
  approvalsPending: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  llmModel: string | null;
  turnCount: number | null;
}

/** Runner detail as the detail endpoint returns it (fields the scenarios need). */
export interface DevMachinesRunnerDetail {
  id: string;
  name: string;
  status: string;
  os: string;
  arch: string;
  runnerVersion: string;
  workingDir: string | null;
  protocolVersion: number;
  capabilities: string[];
  lastHeartbeatAt: string | null;
  owner: string | null;
  podName: string | null;
  podProject: string | null;
  machineLabel: string | null;
  connection: string | null;
  enrolledAt: string | null;
  revokedAt: string | null;
  revokedReason: string;
  createdAt: string | null;
  updatedAt: string | null;
  liveState: DevMachinesLiveState | null;
}

/** Raw outcome of a dev-machine rotate/revoke/delete probe. */
export interface DevMachinesActionResult {
  status: number;
  error: string | null;
}

/** API container for shell snippets; the base helper pins parity19-api. */
function devMachinesApiContainer(): string {
  const override = process.env["PARITY_API_CONTAINER"];
  return override !== undefined && override.trim() !== "" ? override.trim() : "parity19-api";
}

/** Run a Django shell snippet inside this run's stack api container. */
export async function devMachinesShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", devMachinesApiContainer(), "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

function devMachinesText(raw: unknown): string | null {
  return typeof raw === "string" ? raw : null;
}

function devMachinesCount(raw: unknown): number {
  return typeof raw === "number" ? raw : 0;
}

/** Render a value as a Python literal for shell snippets (JSON booleans/null are not). */
function devMachinesPy(value: unknown): string {
  if (value === null || value === undefined) return "None";
  if (value === true) return "True";
  if (value === false) return "False";
  return JSON.stringify(value) ?? "None";
}

/** Workspace UUID for a slug (dev-machine reads key off the id, not the slug). */
export async function serverDevMachinesWorkspaceId(workspaceSlug: string): Promise<string> {
  const out = await devMachinesShell(
    `from pi_dash.db.models import Workspace\n` +
      `ws = Workspace.objects.filter(slug=${JSON.stringify(workspaceSlug)}).first()\n` +
      `print("PARITY_DM_WS:" + (str(ws.id) if ws else ""))\n`
  );
  const id = /^PARITY_DM_WS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error(`[parity] no workspace for slug ${workspaceSlug}.`);
  return id;
}

/** Minimal chat-session shape the chat scenarios assert on. */
export interface RunnerChatSession {
  id: string;
  status: string;
  runner: string;
  last_message_at: string | null;
  active_message_id: string | null;
  active_turn_id: string;
  cwd: string;
}

/** Minimal chat-message shape the chat scenarios assert on. */
export interface RunnerChatMessage {
  id: string;
  role: string;
  content: string;
  status: string;
  seq: number;
}

function runnerChatRunnerOf(raw: unknown): RunnerChatRunner {
  const record = raw as { id?: unknown; name?: unknown; status?: unknown; pod?: unknown; pod_detail?: unknown };
  const podDetail = record.pod_detail as { id?: unknown } | undefined;
  const podId = typeof record.pod === "string" ? record.pod : typeof podDetail?.id === "string" ? podDetail.id : "";
  if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.status !== "string") {
    throw new Error("[parity] runner payload carried no id/name/status.");
  }
  return { id: record.id, name: record.name, status: record.status, podId };
}

function runnerChatSessionOf(raw: unknown): RunnerChatSession {
  const record = raw as {
    id?: unknown;
    status?: unknown;
    runner?: unknown;
    last_message_at?: unknown;
    active_message_id?: unknown;
    active_turn_id?: unknown;
    cwd?: unknown;
  };
  if (typeof record.id !== "string" || typeof record.status !== "string" || typeof record.runner !== "string") {
    throw new Error("[parity] chat-session payload carried no id/status/runner.");
  }
  return {
    id: record.id,
    status: record.status,
    runner: record.runner,
    last_message_at: typeof record.last_message_at === "string" ? record.last_message_at : null,
    active_message_id: typeof record.active_message_id === "string" ? record.active_message_id : null,
    active_turn_id: typeof record.active_turn_id === "string" ? record.active_turn_id : "",
    cwd: typeof record.cwd === "string" ? record.cwd : "",
  };
}

function runnerChatMessageOf(raw: unknown): RunnerChatMessage {
  const record = raw as { id?: unknown; role?: unknown; content?: unknown; status?: unknown; seq?: unknown };
  if (
    typeof record.id !== "string" ||
    typeof record.role !== "string" ||
    typeof record.content !== "string" ||
    typeof record.status !== "string" ||
    typeof record.seq !== "number"
  ) {
    throw new Error("[parity] chat-message payload missed id/role/content/status/seq.");
  }
  return { id: record.id, role: record.role, content: record.content, status: record.status, seq: record.seq };
}

/** Workspace UUID for a slug (runner rows key off the id, not the slug). */
export async function serverWorkspaceIdBySlug(workspaceSlug: string): Promise<string> {
  const out = await runnerChatShell(
    `from pi_dash.db.models import Workspace\n` +
      `ws = Workspace.objects.filter(slug=${JSON.stringify(workspaceSlug)}).first()\n` +
      `print("PARITY_WS:" + (str(ws.id) if ws else ""))\n`
  );
  const id = /^PARITY_WS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error(`[parity] no workspace for slug ${workspaceSlug}.`);
  return id;
}

/**
 * Plant a dev machine owned by `ownerEmail` and attach it to the workspace
 * with a machine token, so the workspace-scoped list shows it even before
 * any runner exists. Labels must be unique per run (callers add a
 * timestamp); the page finds rows by label.
 */
export async function serverDevMachinesPlantMachine(input: {
  ownerEmail: string;
  workspaceSlug: string;
  label: string;
  hostLabel?: string;
  revoked?: boolean;
}): Promise<DevMachinesMachine> {
  const hostLabel = input.hostLabel ?? "";
  const out = await devMachinesShell(
    `import json, secrets\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import DevMachine, MachineToken\n` +
      `user = User.objects.get(email=${JSON.stringify(input.ownerEmail)})\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `machine = DevMachine.objects.create(owner=user, host_label=${JSON.stringify(hostLabel)}, label=${JSON.stringify(input.label)})\n` +
      `MachineToken.objects.create(user=user, workspace=ws, dev_machine=machine, host_label=${JSON.stringify(hostLabel || input.label)}, token_hash=secrets.token_hex(32), label=${JSON.stringify(input.label)})\n` +
      `if ${input.revoked === true ? "True" : "False"}:\n` +
      `    machine.revoked_at = timezone.now()\n` +
      `    machine.save(update_fields=["revoked_at", "updated_at"])\n` +
      `print("PARITY_DM_MACHINE:" + json.dumps({"id": str(machine.id), "label": machine.label, "host_label": machine.host_label, "revoked_at": machine.revoked_at.isoformat() if machine.revoked_at else None}))\n`
  );
  const line = /^PARITY_DM_MACHINE:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] machine plant produced no row for ${input.label}.`);
  const record = JSON.parse(line) as { id: string; label: string; host_label: string; revoked_at: string | null };
  return { id: record.id, label: record.label, hostLabel: record.host_label, revokedAt: record.revoked_at };
}

/**
 * Best-effort machine cleanup: drop the machine's runners first (the FK is
 * SET_NULL, so the machine delete alone would orphan them), then the
 * machine row itself.
 */
export async function serverDevMachinesCleanupMachine(machineId: string): Promise<void> {
  try {
    await devMachinesShell(
      `from pi_dash.runner.models import DevMachine, Runner\n` +
        `Runner.objects.filter(dev_machine_id=${JSON.stringify(machineId)}).delete()\n` +
        `DevMachine.objects.filter(pk=${JSON.stringify(machineId)}).delete()\n` +
        `print("PARITY_DM_CLEANUP_OK")\n`
    );
  } catch (error) {
    console.log(`[parity] machine cleanup failed for ${machineId}; leaving it for reseed. ${String(error)}`);
  }
}

/**
 * Plant a runner row owned by `ownerEmail` in the seeded project, bound to
 * a dev machine when `machineId` is set. Online/busy runners also get a
 * live RunnerSession row: the stack treats a runner with no active session
 * as disconnected, so a planted "online" runner needs the session row to
 * behave like a connected daemon. Heartbeat age is relative to the plant
 * moment; enrolled defaults to set (pass "none" for pending-enrolment).
 */
export async function serverDevMachinesPlantRunner(input: {
  ownerEmail: string;
  workspaceSlug: string;
  projectId: string;
  machineId: string | null;
  name: string;
  status: DevMachinesRunnerStatus;
  os?: string;
  arch?: string;
  runnerVersion?: string;
  workingDir?: string;
  capabilities?: string[];
  lastHeartbeatAgeSecs?: number | null;
  enrolled?: "now" | "none";
  revokedReason?: string;
}): Promise<DevMachinesRunner> {
  const out = await devMachinesShell(
    `import json\n` +
      `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import DevMachine, Pod, Runner, RunnerSession\n` +
      `user = User.objects.get(email=${JSON.stringify(input.ownerEmail)})\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `pod = Pod.default_for_project_id(${JSON.stringify(input.projectId)})\n` +
      `machine = DevMachine.objects.filter(pk=${JSON.stringify(input.machineId ?? "")}).first() if ${input.machineId === null ? "False" : "True"} else None\n` +
      `heartbeat = (timezone.now() - timedelta(seconds=${input.lastHeartbeatAgeSecs ?? "null"})) if ${input.lastHeartbeatAgeSecs === null || input.lastHeartbeatAgeSecs === undefined ? "False" : "True"} else None\n` +
      `runner = Runner.objects.create(owner=user, workspace=ws, pod=pod, dev_machine=machine, name=${JSON.stringify(input.name)}, status=${JSON.stringify(input.status)}, os=${JSON.stringify(input.os ?? "")}, arch=${JSON.stringify(input.arch ?? "")}, runner_version=${JSON.stringify(input.runnerVersion ?? "")}, dev_metadata=${JSON.stringify(input.workingDir === undefined ? {} : { working_dir: input.workingDir })}, capabilities=${JSON.stringify(input.capabilities ?? [])}, last_heartbeat_at=heartbeat, enrolled_at=(timezone.now() if ${input.enrolled === "none" ? "False" : "True"} else None))\n` +
      `if ${input.revokedReason === undefined ? "False" : "True"}:\n` +
      `    runner.revoked_at = timezone.now()\n` +
      `    runner.revoked_reason = ${JSON.stringify(input.revokedReason ?? "")}\n` +
      `    runner.save(update_fields=["revoked_at", "revoked_reason", "updated_at"])\n` +
      `if ${JSON.stringify(input.status)} in ("online", "busy"):\n` +
      `    RunnerSession.objects.create(runner=runner, last_seen_at=timezone.now())\n` +
      `print("PARITY_DM_RUNNER:" + json.dumps({"id": str(runner.id), "name": runner.name, "status": runner.status, "pod": str(runner.pod_id)}))\n`
  );
  const line = /^PARITY_DM_RUNNER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] runner plant produced no row for ${input.name}.`);
  const record = JSON.parse(line) as { id: string; name: string; status: string; pod: string };
  return { id: record.id, name: record.name, status: record.status, podId: record.pod };
}

/** Best-effort runner cleanup; cascades to sessions and the live state. */
export async function serverDevMachinesCleanupRunner(runnerId: string): Promise<void> {
  try {
    await devMachinesShell(
      `from pi_dash.runner.models import Runner\n` +
        `Runner.objects.filter(pk=${JSON.stringify(runnerId)}).delete()\n` +
        `print("PARITY_DM_RUNNER_CLEANUP_OK")\n`
    );
  } catch (error) {
    console.log(`[parity] runner cleanup failed for ${runnerId}; leaving it for reseed. ${String(error)}`);
  }
}

/** The seeded project's default pod id (every runner belongs to one pod). */
export async function serverDefaultPodId(projectId: string): Promise<string> {
  const out = await runnerChatShell(
    `from pi_dash.runner.models import Pod\n` +
      `pod = Pod.default_for_project_id(${JSON.stringify(projectId)})\n` +
      `print("PARITY_POD:" + (str(pod.id) if pod else ""))\n`
  );
  const id = /^PARITY_POD:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error(`[parity] no default pod for project ${projectId}.`);
  return id;
}

/**
 * Plant a runner row owned by `ownerEmail` in the seeded project, at the
 * given status. Names must be unique per run (callers add a timestamp);
 * there is no web create endpoint, so this goes through the shell.
 * Online/busy runners also get a live RunnerSession row: the outbox
 * treats a runner with no active session as offline and rejects
 * chat_warm/chat_cancel/chat_close (the close endpoint then 500s after
 * persisting), so a planted "online" runner needs the session row to
 * behave like a connected daemon. Offline/revoked runners get none.
 */
export async function serverCreateRunner(input: {
  ownerEmail: string;
  workspaceSlug: string;
  projectId: string;
  name: string;
  status: RunnerChatStatus;
}): Promise<RunnerChatRunner> {
  const out = await runnerChatShell(
    `import json\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import Pod, Runner, RunnerSession\n` +
      `user = User.objects.get(email=${JSON.stringify(input.ownerEmail)})\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `pod = Pod.default_for_project_id(${JSON.stringify(input.projectId)})\n` +
      `runner = Runner.objects.create(owner=user, workspace=ws, pod=pod, name=${JSON.stringify(input.name)}, status=${JSON.stringify(input.status)})\n` +
      `live = ${JSON.stringify(input.status)} in ("online", "busy")\n` +
      `if live:\n` +
      `    RunnerSession.objects.create(runner=runner, last_seen_at=timezone.now())\n` +
      `print("PARITY_RUNNER:" + json.dumps({"id": str(runner.id), "name": runner.name, "status": runner.status, "pod": str(runner.pod_id)}))\n`
  );
  const line = /^PARITY_RUNNER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] runner plant produced no row for ${input.name}.`);
  return runnerChatRunnerOf(JSON.parse(line) as unknown);
}

/**
 * Flip a planted runner's status (online/busy/offline/revoked),
 * keeping the live-session row in step: reconnecting revives (or
 * creates) the active session, going offline/revoked revokes it.
 */
export async function serverSetRunnerStatus(runnerId: string, status: RunnerChatStatus): Promise<void> {
  await runnerChatShell(
    `from django.utils import timezone\n` +
      `from pi_dash.runner.models import Runner, RunnerSession\n` +
      `Runner.objects.filter(pk=${JSON.stringify(runnerId)}).update(status=${JSON.stringify(status)})\n` +
      `live = ${JSON.stringify(status)} in ("online", "busy")\n` +
      `now = timezone.now()\n` +
      `active = RunnerSession.objects.filter(runner_id=${JSON.stringify(runnerId)}, revoked_at__isnull=True).first()\n` +
      `if live and active is None:\n` +
      `    RunnerSession.objects.create(runner_id=${JSON.stringify(runnerId)}, last_seen_at=now)\n` +
      `elif not live and active is not None:\n` +
      `    active.revoked_at = now\n` +
      `    active.revoked_reason = "parity_offline"\n` +
      `    active.save(update_fields=["revoked_at", "revoked_reason"])\n` +
      `print("PARITY_RUNNER_STATUS_OK")\n`
  );
}

/** Best-effort runner cleanup; cascades to sessions, messages and events. */
export async function serverCleanupRunner(runnerId: string): Promise<void> {
  try {
    await runnerChatShell(
      `from pi_dash.runner.models import Runner\n` +
        `Runner.objects.filter(pk=${JSON.stringify(runnerId)}).delete()\n` +
        `print("PARITY_RUNNER_CLEANUP_OK")\n`
    );
  } catch (error) {
    console.log(`[parity] runner cleanup failed for ${runnerId}; leaving it for reseed. ${String(error)}`);
  }
}

/**
 * Plant (or replace) a runner's observability snapshot. Event age is
 * relative to the plant moment so badge-threshold scenarios stay exact;
 * pass null for an unknown event time. Token counts ride the canonical
 * usage shape.
 */
export async function serverDevMachinesPlantLiveState(
  runnerId: string,
  input: {
    lastEventAgeSecs: number | null;
    lastEventKind?: string | null;
    lastEventSummary?: string | null;
    agentPid?: number | null;
    subprocessAlive?: boolean | null;
    approvalsPending?: number | null;
    inputTokens?: number | null;
    outputTokens?: number | null;
    totalTokens?: number | null;
    llmModel?: string | null;
    turnCount?: number | null;
  }
): Promise<void> {
  const usage: Record<string, number> = {};
  if (input.inputTokens !== undefined && input.inputTokens !== null) usage["input"] = input.inputTokens;
  if (input.outputTokens !== undefined && input.outputTokens !== null) usage["output"] = input.outputTokens;
  if (input.totalTokens !== undefined && input.totalTokens !== null) usage["total"] = input.totalTokens;
  await devMachinesShell(
    `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.runner.models import Runner, RunnerLiveState\n` +
      `runner = Runner.objects.get(pk=${JSON.stringify(runnerId)})\n` +
      `event_at = (timezone.now() - timedelta(seconds=${input.lastEventAgeSecs ?? "null"})) if ${input.lastEventAgeSecs === null ? "False" : "True"} else None\n` +
      `RunnerLiveState.objects.update_or_create(runner=runner, defaults={"last_event_at": event_at, "last_event_kind": ${devMachinesPy(input.lastEventKind)}, "last_event_summary": ${devMachinesPy(input.lastEventSummary)}, "agent_pid": ${devMachinesPy(input.agentPid)}, "agent_subprocess_alive": ${devMachinesPy(input.subprocessAlive)}, "approvals_pending": ${devMachinesPy(input.approvalsPending)}, "usage": ${devMachinesPy(usage)}, "llm_model": ${devMachinesPy(input.llmModel)}, "turn_count": ${devMachinesPy(input.turnCount)}})\n` +
      `print("PARITY_DM_LIVE_OK")\n`
  );
}

/** Drop a runner's observability snapshot (the unknown-badge fixture). */
export async function serverDevMachinesClearLiveState(runnerId: string): Promise<void> {
  await devMachinesShell(
    `from pi_dash.runner.models import RunnerLiveState\n` +
      `RunnerLiveState.objects.filter(runner_id=${JSON.stringify(runnerId)}).delete()\n` +
      `print("PARITY_DM_LIVE_CLEAR_OK")\n`
  );
}

/** Whether a machine row still exists (delete scenarios assert the drop). */
export async function serverDevMachinesMachineExists(machineId: string): Promise<boolean> {
  const out = await devMachinesShell(
    `from pi_dash.runner.models import DevMachine\n` +
      `print("PARITY_DM_EXISTS:" + ("yes" if DevMachine.objects.filter(pk=${JSON.stringify(machineId)}).exists() else "no"))\n`
  );
  return /^PARITY_DM_EXISTS:(.+)$/m.exec(out)?.[1]?.trim() === "yes";
}

/** Revocation flags of a machine's tokens (rotate invalidates, never deletes). */
export async function serverDevMachinesTokenRevocations(machineId: string): Promise<boolean[]> {
  const out = await devMachinesShell(
    `import json\n` +
      `from pi_dash.runner.models import MachineToken\n` +
      `flags = [t.revoked_at is not None for t in MachineToken.objects.filter(dev_machine_id=${JSON.stringify(machineId)})]\n` +
      `print("PARITY_DM_TOKENS:" + json.dumps(flags))\n`
  );
  const line = /^PARITY_DM_TOKENS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] token probe produced no row for ${machineId}.`);
  return JSON.parse(line) as boolean[];
}

/** Revocation flags of a runner's sessions (rotate/revoke close them). */
export async function serverDevMachinesSessionRevocations(runnerId: string): Promise<boolean[]> {
  const out = await devMachinesShell(
    `import json\n` +
      `from pi_dash.runner.models import RunnerSession\n` +
      `flags = [s.revoked_at is not None for s in RunnerSession.objects.filter(runner_id=${JSON.stringify(runnerId)})]\n` +
      `print("PARITY_DM_SESSIONS:" + json.dumps(flags))\n`
  );
  const line = /^PARITY_DM_SESSIONS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] session probe produced no row for ${runnerId}.`);
  return JSON.parse(line) as boolean[];
}

function devMachinesRowOf(raw: unknown): DevMachinesMachineRow {
  const record = raw as Record<string, unknown>;
  const id = devMachinesText(record["id"]);
  if (id === null) throw new Error("[parity] machine row carried no id.");
  return {
    id,
    label: devMachinesText(record["label"]) ?? "",
    hostLabel: devMachinesText(record["host_label"]) ?? "",
    revokedAt: devMachinesText(record["revoked_at"]),
    runnerCount: devMachinesCount(record["runner_count"]),
    onlineRunnerCount: devMachinesCount(record["online_runner_count"]),
    lastSeenAt: devMachinesText(record["last_seen_at"]),
    lastHeartbeatAt: devMachinesText(record["last_heartbeat_at"]),
  };
}

/** Dev-machines list through the same endpoint the page polls. */
export async function serverDevMachinesListMachines(
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DevMachinesMachineRow[]> {
  const res = await fetchTolerant(`${apiBase}/api/runners/dev-machines/?workspace=${encodeURIComponent(workspaceId)}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] dev-machines list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(devMachinesRowOf);
}

function devMachinesLiveStateOf(raw: unknown): DevMachinesLiveState | null {
  if (raw === null || raw === undefined) return null;
  const record = raw as Record<string, unknown>;
  const num = (value: unknown): number | null => (typeof value === "number" ? value : null);
  const flag = (value: unknown): boolean | null => (typeof value === "boolean" ? value : null);
  return {
    lastEventAt: devMachinesText(record["last_event_at"]),
    lastEventKind: devMachinesText(record["last_event_kind"]),
    lastEventSummary: devMachinesText(record["last_event_summary"]),
    agentPid: num(record["agent_pid"]),
    subprocessAlive: flag(record["agent_subprocess_alive"]),
    approvalsPending: num(record["approvals_pending"]),
    inputTokens: num(record["input_tokens"]),
    outputTokens: num(record["output_tokens"]),
    totalTokens: num(record["total_tokens"]),
    llmModel: devMachinesText(record["llm_model"]),
    turnCount: num(record["turn_count"]),
  };
}

function devMachinesDetailOf(raw: unknown): DevMachinesRunnerDetail {
  const record = raw as Record<string, unknown>;
  const id = devMachinesText(record["id"]);
  const name = devMachinesText(record["name"]);
  const status = devMachinesText(record["status"]);
  if (id === null || name === null || status === null) {
    throw new Error("[parity] runner detail carried no id/name/status.");
  }
  const meta = record["dev_metadata"] as Record<string, unknown> | undefined;
  const pod = record["pod_detail"] as Record<string, unknown> | null | undefined;
  const machine = record["dev_machine_detail"] as Record<string, unknown> | null | undefined;
  const caps: unknown = record["capabilities"];
  return {
    id,
    name,
    status,
    os: devMachinesText(record["os"]) ?? "",
    arch: devMachinesText(record["arch"]) ?? "",
    runnerVersion: devMachinesText(record["runner_version"]) ?? "",
    workingDir: meta === undefined || meta === null ? null : (devMachinesText(meta["working_dir"]) ?? null),
    protocolVersion: typeof record["protocol_version"] === "number" ? record["protocol_version"] : 0,
    capabilities: Array.isArray(caps) ? caps.filter((entry): entry is string => typeof entry === "string") : [],
    lastHeartbeatAt: devMachinesText(record["last_heartbeat_at"]),
    owner: devMachinesText(record["owner"]),
    podName: pod === null || pod === undefined ? null : (devMachinesText(pod["name"]) ?? null),
    podProject: pod === null || pod === undefined ? null : (devMachinesText(pod["project_identifier"]) ?? null),
    machineLabel:
      machine === null || machine === undefined
        ? null
        : (devMachinesText(machine["label"]) ?? devMachinesText(machine["host_label"])),
    connection: devMachinesText(record["connection"]),
    enrolledAt: devMachinesText(record["enrolled_at"]),
    revokedAt: devMachinesText(record["revoked_at"]),
    revokedReason: devMachinesText(record["revoked_reason"]) ?? "",
    createdAt: devMachinesText(record["created_at"]),
    updatedAt: devMachinesText(record["updated_at"]),
    liveState: devMachinesLiveStateOf(record["live_state"]),
  };
}

/**
 * Runner detail through the same endpoint the detail page polls.
 * Resolves the status so delete scenarios can assert the 404.
 */
export async function serverDevMachinesRunnerDetail(
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; detail: DevMachinesRunnerDetail | null }> {
  const res = await fetchTolerant(`${apiBase}/api/runners/${encodeURIComponent(runnerId)}/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return { status: 404, detail: null };
  if (!res.ok) throw new Error(`[parity] runner detail failed with HTTP ${res.status}.`);
  return { status: res.status, detail: devMachinesDetailOf((await res.json()) as unknown) };
}

async function devMachinesActionOf(res: Response): Promise<DevMachinesActionResult> {
  let error: string | null = null;
  if (!res.ok) {
    try {
      const payload = (await res.json()) as Record<string, unknown>;
      error = devMachinesText(payload["error"]);
    } catch {
      error = null;
    }
  }
  return { status: res.status, error };
}

/** Rotate probe through the app's own endpoint (409 on revoked machines). */
export async function serverDevMachinesRotate(
  machineId: string,
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DevMachinesActionResult> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/dev-machines/${machineId}/rotate/`, sessionCookie, {
    workspace: workspaceId,
  });
  return devMachinesActionOf(res);
}

/** Revoke probe through the app's own endpoint. */
export async function serverDevMachinesRevoke(
  machineId: string,
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DevMachinesActionResult> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/dev-machines/${machineId}/revoke/`, sessionCookie, {
    workspace: workspaceId,
  });
  return devMachinesActionOf(res);
}

/** Delete probe through the app's own endpoint (204 on success). */
export async function serverDevMachinesDelete(
  machineId: string,
  workspaceId: string,
  purgeLocal: boolean,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DevMachinesActionResult> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/runners/dev-machines/${machineId}/?workspace=${encodeURIComponent(workspaceId)}&purge_local=${purgeLocal ? "true" : "false"}`,
    sessionCookie
  );
  return devMachinesActionOf(res);
}

/** Binding patch payload; scheduler/project stay out (uninstall + reinstall to swap). */
export type ParitySchedulerBindingPatch = Partial<
  Pick<
    ParitySchedulerBinding,
    "dtstart" | "tzid" | "rrule" | "rdates" | "exdates" | "extra_context" | "enabled" | "outcome_mode" | "pod"
  >
>;

/** Patch an install (project admin only); resolves with the detail-shaped row. */
export async function serverPatchBinding(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  patch: ParitySchedulerBindingPatch,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerBindingDetail> {
  const res = await serverPatchBindingStatus(workspaceSlug, projectId, bindingId, sessionCookie, patch, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] binding patch failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParitySchedulerBindingDetail;
}

/** Patch an install; resolves with the outcome instead of throwing on 4xx. */
export async function serverPatchBindingStatus(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  patch: ParitySchedulerBindingPatch,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/`,
    sessionCookie,
    patch,
    apiBase
  );
}

/**
 * Convergent install builder: returns the live row when `input.scheduler` is
 * already bound to the project (a retry reuses it), otherwise installs it. A
 * create that loses an install race re-lists instead of throwing.
 */
export async function ensureBinding(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  input: ParitySchedulerBindingInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerBinding> {
  const existing = (await serverBindings(workspaceSlug, projectId, sessionCookie, apiBase)).find(
    (row) => row.scheduler === input.scheduler
  );
  if (existing !== undefined) return existing;
  const created = await serverCreateBindingStatus(workspaceSlug, projectId, sessionCookie, input, apiBase);
  if (created.status === 201) return created.payload as ParitySchedulerBinding;
  const raced = (await serverBindings(workspaceSlug, projectId, sessionCookie, apiBase)).find(
    (row) => row.scheduler === input.scheduler
  );
  if (raced !== undefined) return raced;
  throw new Error(`[parity] binding create failed with HTTP ${created.status}: ${JSON.stringify(created.payload)}`);
}

/** One AgentRun row as the binding run-history read returns it. */
export interface ParityAgentRun {
  id: string;
  created_at: string;
  started_at: string | null;
  ended_at: string | null;
  status: string;
  error: string;
  done_payload: Record<string, unknown> | null;
  pod_detail: { name: string } | null;
}

/** Paged run-history envelope (same shape as the runners run listing). */
export interface ParityAgentRunPage {
  results: ParityAgentRun[];
  total_count: number;
  total_pages: number;
  page: number;
  per_page: number;
  count: number;
}

/** Read one install's run history page (any project role may read). */
export async function serverBindingRuns(
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  sessionCookie: string,
  page = 1,
  perPage?: number,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityAgentRunPage> {
  const params = new URLSearchParams({ page: String(page) });
  if (perPage !== undefined) params.set("per_page", String(perPage));
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/${bindingId}/runs/?${params.toString()}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] binding runs read failed with HTTP ${res.status}.`);
  }
  return res.payload as ParityAgentRunPage;
}

/** One calendar occurrence: a past run or an RRULE-expanded future firing. */
export interface ParitySchedulerOccurrence {
  binding_id: string;
  scheduler_id: string;
  scheduler_name: string;
  scheduler_color: string;
  dtstart: string;
  tzid: string;
  kind: "scheduled" | "past";
  agent_run_id: string | null;
  status: string | null;
}

/** Occurrences window read with its truncation hint. */
export interface ParitySchedulerOccurrenceResponse {
  occurrences: ParitySchedulerOccurrence[];
  has_more: boolean;
  next_window_start: string | null;
}

/** Read a project's firing occurrences in a window (any project role may read). */
export async function serverOccurrences(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  from: string,
  to: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParitySchedulerOccurrenceResponse> {
  const params = new URLSearchParams({ from, to });
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/scheduler-bindings/occurrences/?${params.toString()}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] occurrences read failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParitySchedulerOccurrenceResponse;
}

/** This session's user id (for seating a provisioned member onto a project). */
export async function serverSessionUserId(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await apiJson("GET", "/api/users/me/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] users/me read failed with HTTP ${res.status}.`);
  }
  const id = (res.payload as { id?: unknown }).id;
  if (typeof id !== "string" || id === "") throw new Error("[parity] users/me carried no id.");
  return id;
}

/** Runner detail through the same endpoint the chat header reads. */
export async function serverRunner(
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatRunner> {
  const res = await fetchTolerant(`${apiBase}/api/runners/${runnerId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] runner read failed with HTTP ${res.status}.`);
  return runnerChatRunnerOf((await res.json()) as unknown);
}

/**
 * Create a chat session through the app's own endpoint. The endpoint
 * reuses an open message-less session with HTTP 200 instead of creating
 * a duplicate, so both 200 (reused) and 201 (created) resolve.
 */
export async function serverCreateChatSession(
  input: { workspaceId: string; runnerId: string },
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatSession> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/`, sessionCookie, {
    workspace: input.workspaceId,
    runner: input.runnerId,
  });
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] chat-session create failed with HTTP ${res.status}.`);
  }
  return runnerChatSessionOf((await res.json()) as unknown);
}

/** Chat sessions through the same endpoint the history panel reads. */
export async function serverListChatSessions(
  workspaceId: string,
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatSession[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/runners/chat/sessions/?workspace=${encodeURIComponent(workspaceId)}&runner=${encodeURIComponent(runnerId)}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] chat-session list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(runnerChatSessionOf);
}

/** One chat session through the app's own endpoint. */
export async function serverGetChatSession(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatSession> {
  const res = await fetchTolerant(`${apiBase}/api/runners/chat/sessions/${sessionId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] chat-session read failed with HTTP ${res.status}.`);
  return runnerChatSessionOf((await res.json()) as unknown);
}

/** Messages of a session through the app's own endpoint. */
export async function serverChatMessages(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatMessage[]> {
  const res = await fetchTolerant(`${apiBase}/api/runners/chat/sessions/${sessionId}/messages/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] chat-message list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map(runnerChatMessageOf);
}

/**
 * Send a chat message through the app's own endpoint. Resolves with the
 * stored user message; the session is left mid-turn (active message set),
 * exactly as after a UI send with no daemon to answer it.
 */
export async function serverSendChatMessage(
  sessionId: string,
  content: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatMessage> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/messages/`, sessionCookie, {
    content,
    content_parts: [],
  });
  if (res.status !== 201) throw new Error(`[parity] chat send failed with HTTP ${res.status}.`);
  return runnerChatMessageOf((await res.json()) as unknown);
}

/** Close a chat session through the app's own endpoint. */
export async function serverCloseChatSession(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<RunnerChatSession> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/close/`, sessionCookie, {});
  if (!res.ok) throw new Error(`[parity] chat close failed with HTTP ${res.status}.`);
  return runnerChatSessionOf((await res.json()) as unknown);
}

/**
 * Craft session states the REST surface cannot produce (ordering stamps,
 * parked or cleared turns). Only the named fields are touched.
 */
export async function serverSetChatSession(
  sessionId: string,
  fields: {
    lastMessageAtIso?: string | null;
    activeTurnId?: string;
    activeMessageId?: string | null;
    status?: "open" | "closed";
  }
): Promise<void> {
  const sets: string[] = [];
  if (fields.lastMessageAtIso !== undefined) {
    sets.push(
      fields.lastMessageAtIso === null
        ? `session.last_message_at = None`
        : `session.last_message_at = parse_datetime(${JSON.stringify(fields.lastMessageAtIso)})`
    );
  }
  if (fields.activeTurnId !== undefined) sets.push(`session.active_turn_id = ${JSON.stringify(fields.activeTurnId)}`);
  if (fields.activeMessageId !== undefined) {
    sets.push(
      fields.activeMessageId === null
        ? `session.active_message_id = None`
        : `session.active_message_id = ${JSON.stringify(fields.activeMessageId)}`
    );
  }
  if (fields.status !== undefined) sets.push(`session.status = ${JSON.stringify(fields.status)}`);
  await runnerChatShell(
    `from django.utils.dateparse import parse_datetime\n` +
      `from pi_dash.runner.models import AgentChatSession\n` +
      `session = AgentChatSession.objects.get(pk=${JSON.stringify(sessionId)})\n` +
      sets.map((line) => `${line}\n`).join("") +
      `session.save()\n` +
      `print("PARITY_CHAT_SESSION_OK")\n`
  );
}

/** Persisted event kinds of a session in sequence order (server-side stream log). */
export async function serverChatEventKinds(sessionId: string): Promise<{ seq: number; kind: string }[]> {
  const out = await runnerChatShell(
    `import json\n` +
      `from pi_dash.runner.models import AgentChatEvent\n` +
      `rows = [{"seq": e.seq, "kind": e.kind} for e in AgentChatEvent.objects.filter(session_id=${JSON.stringify(sessionId)}).order_by("seq")]\n` +
      `print("PARITY_CHAT_EVENTS:" + json.dumps(rows))\n`
  );
  const line = /^PARITY_CHAT_EVENTS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] chat-event read produced no row for ${sessionId}.`);
  return JSON.parse(line) as { seq: number; kind: string }[];
}

/**
 * Read the live SSE endpoint until `maxChars` arrive or `waitMs` elapse,
 * then cancel the (never-ending) stream. Proves the resumable replay:
 * `after=0` replays persisted frames, `after=<tip>` stays quiet.
 */
export async function serverReadChatStream(
  sessionId: string,
  sessionCookie: string,
  after: number,
  options?: { maxChars?: number; waitMs?: number }
): Promise<string> {
  const apiBase = apiBaseFromEnv();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), options?.waitMs ?? 8000);
  try {
    const res = await fetch(`${apiBase}/api/runners/chat/sessions/${sessionId}/events/?after=${after}`, {
      headers: { cookie: sessionCookie },
      signal: controller.signal,
    });
    if (!res.ok || !res.body) throw new Error(`[parity] chat SSE read failed with HTTP ${res.status}.`);
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let text = "";
    const limit = options?.maxChars ?? 4000;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      text += decoder.decode(value, { stream: true });
      if (text.length >= limit) break;
    }
    await reader.cancel();
    return text;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") return "";
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

// ---------------------------------------------------------------------------
// Prompt sections + compiled receipts + project automations (NEWFRONT-186,
// rows AGT-023..037). Added additively; existing helpers above are untouched.
// Prompt endpoints carry no trailing slash (prompting/urls.py); PUT takes the
// scope in the body while DELETE takes it as a query param.
// ---------------------------------------------------------------------------

/** Prompt-section resolution scope, mirroring the backend. */
export type ParityPromptScope = "workspace" | "user";

/** Prompt kinds the /prompts page browses. */
export type ParityPromptKind = "coding-task" | "review" | "scheduler";

/** One resolved prompt section, as the list endpoint returns it. */
export interface ParityPromptSection {
  key: string;
  title: string;
  customizable: "locked" | "workspace" | "overridable";
  body: string;
  default_body: string;
  source: string;
  version: number;
  needs_attention: boolean;
  editable_at_workspace: boolean;
  editable_at_personal: boolean;
}

/** Ordered resolved sections for one kind + scope. */
export async function serverPromptSections(
  workspaceSlug: string,
  kind: string,
  scope: ParityPromptScope,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPromptSection[]> {
  const params = new URLSearchParams({ kind, scope });
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/prompt-sections?${params.toString()}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] prompt-sections read failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  const sections = (res.payload as { sections?: unknown }).sections;
  if (!Array.isArray(sections)) throw new Error("[parity] prompt-sections carried no sections array.");
  return sections as ParityPromptSection[];
}

/** A stored section override row, as the upsert endpoint returns it. */
export interface ParityPromptOverride {
  id: string;
  workspace: string;
  user: string | null;
  section_key: string;
  body: string;
  is_active: boolean;
  version: number;
  needs_attention: boolean;
  is_workspace_level: boolean;
}

/** Upsert a section override at a scope (workspace writes are admin-gated). */
export async function serverPromptSectionUpsert(
  workspaceSlug: string,
  sectionKey: string,
  scope: ParityPromptScope,
  body: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPromptOverride> {
  const res = await serverPromptSectionUpsertStatus(workspaceSlug, sectionKey, scope, body, sessionCookie, apiBase);
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] prompt-section upsert failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParityPromptOverride;
}

/**
 * CSRF-paired sibling of apiJson. The prompting endpoints authenticate with
 * DRF's stock session auth, which enforces CSRF (bug NEWFRONT-193: the
 * sibling REST views use the exempt base). These helpers pair the session
 * with a fresh token so they prove the endpoints' own role/tier behavior
 * instead of the CSRF gate; the UI specs assert the headerless reality.
 */
async function apiJsonCsrf(
  method: string,
  path: string,
  sessionCookie: string,
  body?: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const { token, cookie } = await csrfPair(apiBase);
  const res = await fetchWithRetry(`${apiBase}${path}`, {
    method,
    headers: {
      cookie: `${sessionCookie}; ${cookie}`,
      "X-CSRFToken": token,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await res.text();
  let payload: unknown = null;
  try {
    payload = text === "" ? null : (JSON.parse(text) as unknown);
  } catch {
    payload = text;
  }
  return { status: res.status, payload };
}

/** Upsert a section override; resolves with the outcome instead of throwing on 4xx. */
export async function serverPromptSectionUpsertStatus(
  workspaceSlug: string,
  sectionKey: string,
  scope: ParityPromptScope,
  body: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJsonCsrf(
    "PUT",
    `/api/workspaces/${workspaceSlug}/prompt-sections/${sectionKey}`,
    sessionCookie,
    { scope, body },
    apiBase
  );
}

/** Revert (deactivate) the override at a scope; 204 on success. */
export async function serverPromptSectionRevert(
  workspaceSlug: string,
  sectionKey: string,
  scope: ParityPromptScope,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await serverPromptSectionRevertStatus(workspaceSlug, sectionKey, scope, sessionCookie, apiBase);
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] prompt-section revert failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
}

/** Revert an override; resolves with the outcome instead of throwing on 4xx. */
export async function serverPromptSectionRevertStatus(
  workspaceSlug: string,
  sectionKey: string,
  scope: ParityPromptScope,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJsonCsrf(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/prompt-sections/${sectionKey}?scope=${scope}`,
    sessionCookie,
    undefined,
    apiBase
  );
}

/** The assembled template for one kind, plus the automatic-run variant when set. */
export interface ParityPromptCompiled {
  kind: string;
  scope: ParityPromptScope;
  template_body: string;
  automatic_template_body?: string;
}

/** Read the compiled receipt for one kind + scope. */
export async function serverPromptCompiled(
  workspaceSlug: string,
  kind: string,
  scope: ParityPromptScope,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPromptCompiled> {
  const params = new URLSearchParams({ scope });
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/prompts/${kind}/compiled?${params.toString()}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] prompt compiled read failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as ParityPromptCompiled;
}

/** Preview payload: a render target plus an optional unsaved draft. */
export interface ParityPromptPreviewInput {
  issue_id?: string;
  binding_id?: string;
  scope?: ParityPromptScope;
  section_key?: string;
  body?: string;
}

/** Render a prompt against a real issue or install. */
export async function serverPromptPreview(
  workspaceSlug: string,
  kind: string,
  input: ParityPromptPreviewInput,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ kind: string; prompt: string }> {
  const res = await serverPromptPreviewStatus(workspaceSlug, kind, input, sessionCookie, apiBase);
  if (res.status !== 200) {
    throw new Error(`[parity] prompt preview failed with HTTP ${res.status}: ${JSON.stringify(res.payload)}`);
  }
  return res.payload as { kind: string; prompt: string };
}

/** Render a prompt; resolves with the outcome instead of throwing on 4xx. */
export async function serverPromptPreviewStatus(
  workspaceSlug: string,
  kind: string,
  input: ParityPromptPreviewInput,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJsonCsrf("POST", `/api/workspaces/${workspaceSlug}/prompts/${kind}/preview`, sessionCookie, input, apiBase);
}

/** Project automation fields, as the project detail endpoint returns them. */
export interface ParityProjectAutomations {
  archive_in: number;
  close_in: number;
  default_state: string | null;
}

/** Read a project's idle-automation settings (any project role may read). */
export async function serverProjectAutomations(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectAutomations> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] project automations read failed with HTTP ${res.status}.`);
  }
  const record = res.payload as { archive_in?: unknown; close_in?: unknown; default_state?: unknown };
  if (typeof record.archive_in !== "number" || typeof record.close_in !== "number") {
    throw new Error("[parity] project carried no numeric archive_in/close_in.");
  }
  if (record.default_state !== null && typeof record.default_state !== "string") {
    throw new Error("[parity] project carried a non-string default_state.");
  }
  return { archive_in: record.archive_in, close_in: record.close_in, default_state: record.default_state };
}

/** Persist automation fields through the project update (project admin only). */
export async function serverPatchProjectAutomations(
  workspaceSlug: string,
  projectId: string,
  patch: Partial<ParityProjectAutomations>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectAutomations> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    patch,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] project automations patch failed with HTTP ${res.status}.`);
  }
  return serverProjectAutomations(workspaceSlug, projectId, sessionCookie, apiBase);
}

/** Cancelled-group project states (the auto-close target pool). */
export async function serverProjectCancelledStates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/states/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(`[parity] project states read failed with HTTP ${res.status}.`);
  }
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return (rows as { id?: unknown; name?: unknown; group?: unknown }[])
    .filter((row) => row.group === "cancelled")
    .map((row) => {
      if (typeof row.id !== "string" || typeof row.name !== "string") {
        throw new Error("[parity] state row carried no string id/name.");
      }
      return { id: row.id, name: row.name };
    });
}

// --- Add-runner oracle helpers (NEWFRONT-179, RUN-006–009). Appended;
// --- existing helpers above are untouched per the shared harness contract.
// --- Shell snippets reuse devMachinesShell (env-aware container); machine
// --- rows reuse serverDevMachinesPlantMachine/CleanupMachine.

/**
 * Open a fresh control session for a planted machine so the dev-machines
 * list reports it control-online. Resolves with the session id.
 */
export async function serverAddRunnerPlantSession(machineId: string): Promise<string> {
  const out = await devMachinesShell(
    `import json\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.runner.models import DevMachine, MachineSession\n` +
      `machine = DevMachine.objects.get(pk=${JSON.stringify(machineId)})\n` +
      `session = MachineSession.objects.create(dev_machine=machine, last_seen_at=timezone.now())\n` +
      `print("PARITY_AR_SESSION:" + json.dumps({"id": str(session.id)}))\n`
  );
  const line = /^PARITY_AR_SESSION:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] session plant produced no row for machine ${machineId}.`);
  return (JSON.parse(line) as { id: string }).id;
}

/** Refresh a machine's control session to right now (the 90s online window can lapse during a slow sign-in). */
export async function serverAddRunnerTouchSession(machineId: string): Promise<void> {
  await devMachinesShell(
    `from django.utils import timezone\n` +
      `from pi_dash.runner.models import MachineSession\n` +
      `MachineSession.objects.filter(dev_machine_id=${JSON.stringify(machineId)}, revoked_at__isnull=True).update(last_seen_at=timezone.now())\n` +
      `print("PARITY_AR_TOUCH_OK")\n`
  );
}

/** Age a machine's control session by `ageSecs` (dropping it offline past 90s). */
export async function serverAddRunnerAgeSession(machineId: string, ageSecs: number): Promise<void> {
  await devMachinesShell(
    `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.runner.models import MachineSession\n` +
      `MachineSession.objects.filter(dev_machine_id=${JSON.stringify(machineId)}, revoked_at__isnull=True).update(last_seen_at=(timezone.now() - timedelta(seconds=${ageSecs})))\n` +
      `print("PARITY_AR_AGE_OK")\n`
  );
}

/** Drop a machine's control sessions so delivery fails with machine_offline. */
export async function serverAddRunnerDropSession(machineId: string): Promise<void> {
  await devMachinesShell(
    `from pi_dash.runner.models import MachineSession\n` +
      `MachineSession.objects.filter(dev_machine_id=${JSON.stringify(machineId)}).delete()\n` +
      `print("PARITY_AR_DROP_OK")\n`
  );
}

/**
 * Write a daemon result for a create-runner request (the scratch stack runs
 * no live daemon, so the spec stands in for the write-back exactly as the
 * daemon endpoint would store it).
 */
export async function serverAddRunnerSetResult(requestId: string, payload: Record<string, unknown>): Promise<void> {
  await devMachinesShell(
    `import json\n` +
      `from pi_dash.runner.services.machine_outbox import set_command_result\n` +
      `set_command_result(${JSON.stringify(requestId)}, json.loads(${JSON.stringify(JSON.stringify(payload))}))\n` +
      `print("PARITY_AR_RESULT_OK")\n`
  );
}

/** Raw outcome of a create-runner POST (status + parsed body). */
export interface AddRunnerCreateResult {
  status: number;
  body: unknown;
}

/** Drive the create-runner POST directly (advisory-model acceptance proof). */
export async function serverAddRunnerCreate(
  machineId: string,
  workspaceId: string,
  body: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AddRunnerCreateResult> {
  const res = await authedApi(
    `/runners/dev-machines/${machineId}/create-runner/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workspace: workspaceId, ...body }),
    },
    apiBase
  );
  const parsed: unknown = await res.json().catch(() => null);
  return { status: res.status, body: parsed };
}

/** Raw outcome of a create-runner status GET (status + parsed body). */
export interface AddRunnerStatusResult {
  status: number;
  body: Record<string, unknown>;
}

/** Read a create-runner request's server-side result. */
export async function serverAddRunnerStatus(
  machineId: string,
  requestId: string,
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AddRunnerStatusResult> {
  const res = await authedApi(
    `/runners/dev-machines/${machineId}/create-runner/${requestId}/?workspace=${encodeURIComponent(workspaceId)}`,
    sessionCookie,
    undefined,
    apiBase
  );
  const parsed = (await res.json().catch(() => null)) as Record<string, unknown> | null;
  return { status: res.status, body: parsed ?? {} };
}

/** Runner names in the workspace (manual-path-makes-no-server-write proof). */
export async function serverAddRunnerRunnerNames(
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetchTolerant(`${apiBase}/api/runners/?workspace=${encodeURIComponent(workspaceId)}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] runner list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map((row) => String((row as Record<string, unknown>)["name"] ?? "")).filter((name) => name !== "");
}

// ---------------------------------------------------------------------------
// Assistant chat core (NEWFRONT-187): provider/speech key config, threads,
// transcript reads, sends, cancels, and tool-server registry rows. Raw
// variants return status plus payload for the negative halves; strict
// variants throw unless the golden status lands.
// ---------------------------------------------------------------------------

/** One assistant thread as the thread endpoints return it. */
export interface AssistantThread {
  id: string;
  title: string;
  is_archived: boolean;
  has_active_turn: boolean;
  created_at: string;
  updated_at: string;
}

/** One transcript row as the message endpoints return it. */
export interface AssistantMessage {
  id: string;
  role: string;
  content: string;
  status: string;
  seq: number;
  turn_id: string | null;
  payload: Record<string, unknown>;
  created_at: string;
  completed_at: string | null;
}

/** Provider-key config as the config endpoint returns it (never the key). */
export interface AssistantConfig {
  provider_kind: string;
  base_url: string;
  model_name: string;
  has_api_key: boolean;
  last_verified_at: string | null;
}

/** Speech-key config as the stt-config endpoint returns it. */
export interface AssistantSttConfig {
  base_url: string;
  model_name: string;
  has_api_key: boolean;
  last_verified_at: string | null;
}

/** One tool-server registry row. */
export interface AssistantMcpServer {
  id: string;
  name: string;
  url: string;
  has_auth_header: boolean;
  is_enabled: boolean;
}

function assistantThreadOf(row: unknown): AssistantThread {
  const thread = row as Record<string, unknown>;
  if (typeof thread["id"] !== "string" || typeof thread["title"] !== "string") {
    throw new Error("[parity] thread row carried no string id/title.");
  }
  return {
    id: thread["id"],
    title: thread["title"],
    is_archived: thread["is_archived"] === true,
    has_active_turn: thread["has_active_turn"] === true,
    created_at: typeof thread["created_at"] === "string" ? thread["created_at"] : "",
    updated_at: typeof thread["updated_at"] === "string" ? thread["updated_at"] : "",
  };
}

function assistantMessageOf(row: unknown): AssistantMessage {
  const message = row as Record<string, unknown>;
  if (
    typeof message["id"] !== "string" ||
    typeof message["role"] !== "string" ||
    typeof message["status"] !== "string" ||
    typeof message["seq"] !== "number"
  ) {
    throw new Error("[parity] transcript row carried no id/role/status/seq.");
  }
  return {
    id: message["id"],
    role: message["role"],
    content: typeof message["content"] === "string" ? message["content"] : "",
    status: message["status"],
    seq: message["seq"],
    turn_id: typeof message["turn_id"] === "string" ? message["turn_id"] : null,
    payload:
      typeof message["payload"] === "object" && message["payload"] !== null
        ? (message["payload"] as Record<string, unknown>)
        : {},
    created_at: typeof message["created_at"] === "string" ? message["created_at"] : "",
    completed_at: typeof message["completed_at"] === "string" ? message["completed_at"] : null,
  };
}

function assistantConfigOf(payload: unknown): AssistantConfig {
  const config = payload as Record<string, unknown>;
  return {
    provider_kind: typeof config["provider_kind"] === "string" ? config["provider_kind"] : "",
    base_url: typeof config["base_url"] === "string" ? config["base_url"] : "",
    model_name: typeof config["model_name"] === "string" ? config["model_name"] : "",
    has_api_key: config["has_api_key"] === true,
    last_verified_at: typeof config["last_verified_at"] === "string" ? config["last_verified_at"] : null,
  };
}

function assistantSttConfigOf(payload: unknown): AssistantSttConfig {
  const config = payload as Record<string, unknown>;
  return {
    base_url: typeof config["base_url"] === "string" ? config["base_url"] : "",
    model_name: typeof config["model_name"] === "string" ? config["model_name"] : "",
    has_api_key: config["has_api_key"] === true,
    last_verified_at: typeof config["last_verified_at"] === "string" ? config["last_verified_at"] : null,
  };
}

function assistantMcpServerOf(row: unknown): AssistantMcpServer {
  const server = row as Record<string, unknown>;
  if (typeof server["id"] !== "string" || typeof server["name"] !== "string") {
    throw new Error("[parity] tool-server row carried no string id/name.");
  }
  return {
    id: server["id"],
    name: server["name"],
    url: typeof server["url"] === "string" ? server["url"] : "",
    has_auth_header: server["has_auth_header"] === true,
    is_enabled: server["is_enabled"] === true,
  };
}

/** Read the caller's provider-key config (200 with has_api_key, never the key). */
export async function serverAssistantConfigGet(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantConfig> {
  const res = await apiJson("GET", "/api/users/me/ai-assistant/config/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] config read failed with HTTP ${res.status}.`);
  return assistantConfigOf(res.payload);
}

/**
 * Save the caller's provider-key config. Dummy values only — never a real
 * key; scenarios restore with serverAssistantConfigDelete afterwards.
 */
export async function serverAssistantConfigPut(
  sessionCookie: string,
  input: { provider_kind: string; base_url: string; model_name: string; api_key?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantConfig> {
  const res = await mutateJSON("PUT", `${apiBase}/api/users/me/ai-assistant/config/`, sessionCookie, input);
  if (res.status !== 200) throw new Error(`[parity] config save failed with HTTP ${res.status}.`);
  return assistantConfigOf((await res.json()) as unknown);
}

/** Remove the caller's provider-key config (restores the keyless state). */
export async function serverAssistantConfigDelete(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("DELETE", `${apiBase}/api/users/me/ai-assistant/config/`, sessionCookie);
  if (res.status !== 204) throw new Error(`[parity] config delete failed with HTTP ${res.status}.`);
}

/** Read the caller's speech-key config. */
export async function serverAssistantSttGet(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantSttConfig> {
  const res = await apiJson("GET", "/api/users/me/ai-assistant/stt-config/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] stt-config read failed with HTTP ${res.status}.`);
  return assistantSttConfigOf(res.payload);
}

/** Save the caller's speech-key config (dummy values only, restored after). */
export async function serverAssistantSttPut(
  sessionCookie: string,
  input: { base_url: string; model_name: string; api_key?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantSttConfig> {
  const res = await mutateJSON("PUT", `${apiBase}/api/users/me/ai-assistant/stt-config/`, sessionCookie, input);
  if (res.status !== 200) throw new Error(`[parity] stt-config save failed with HTTP ${res.status}.`);
  return assistantSttConfigOf((await res.json()) as unknown);
}

/** Remove the caller's speech-key config. */
export async function serverAssistantSttDelete(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("DELETE", `${apiBase}/api/users/me/ai-assistant/stt-config/`, sessionCookie);
  if (res.status !== 204) throw new Error(`[parity] stt-config delete failed with HTTP ${res.status}.`);
}

/** List the caller's chat threads in a workspace (newest first). */
export async function serverAssistantThreads(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantThread[]> {
  const res = await serverAssistantThreadsRaw(workspaceSlug, sessionCookie, apiBase);
  if (res.status !== 200) throw new Error(`[parity] thread list failed with HTTP ${res.status}.`);
  if (!Array.isArray(res.payload)) throw new Error("[parity] thread list carried no array.");
  return res.payload.map(assistantThreadOf);
}

/** Raw thread list for the guest-refusal halves (403 role_not_allowed). */
export async function serverAssistantThreadsRaw(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson("GET", `/api/workspaces/${workspaceSlug}/ai-assistant/threads/`, sessionCookie, undefined, apiBase);
}

/** Create an untitled chat thread. */
export async function serverAssistantThreadCreate(
  workspaceSlug: string,
  sessionCookie: string,
  title = "",
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantThread> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/`,
    sessionCookie,
    { title }
  );
  if (res.status !== 201) throw new Error(`[parity] thread create failed with HTTP ${res.status}.`);
  return assistantThreadOf((await res.json()) as unknown);
}

/** Raw thread create for the guest-refusal halves. */
export async function serverAssistantThreadCreateRaw(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/`,
    sessionCookie,
    { title: "" }
  );
  return { status: res.status, payload: (await res.text()) as unknown };
}

/** Patch a thread's title / archived flag (archived fixtures for the sidebar). */
export async function serverAssistantThreadPatch(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  patch: { title?: string; is_archived?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantThread> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/`,
    sessionCookie,
    patch
  );
  if (res.status !== 200) throw new Error(`[parity] thread patch failed with HTTP ${res.status}.`);
  return assistantThreadOf((await res.json()) as unknown);
}

/** Read one transcript page (cursor `after`, explicit `limit` for paging). */
export async function serverAssistantMessages(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  after = 0,
  limit = 100,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantMessage[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/messages/?after=${after}&limit=${limit}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] transcript read failed with HTTP ${res.status}.`);
  if (!Array.isArray(res.payload)) throw new Error("[parity] transcript read carried no array.");
  return res.payload.map(assistantMessageOf);
}

/** Send a chat message; resolves with the 202 turn plus the user row. */
export async function serverAssistantSend(
  workspaceSlug: string,
  threadId: string,
  content: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ turnId: string; message: AssistantMessage }> {
  const res = await serverAssistantSendRaw(workspaceSlug, threadId, content, sessionCookie, apiBase);
  if (res.status !== 202) throw new Error(`[parity] send failed with HTTP ${res.status}.`);
  const payload = res.payload as { turn?: { id?: unknown }; message?: unknown };
  if (typeof payload.turn?.id !== "string" || payload.message === undefined) {
    throw new Error("[parity] send response carried no turn/message.");
  }
  return { turnId: payload.turn.id, message: assistantMessageOf(payload.message) };
}

/** Raw send for the refusal halves (400 blank, 409 busy, 422 keyless). */
export async function serverAssistantSendRaw(
  workspaceSlug: string,
  threadId: string,
  content: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/messages/`,
    sessionCookie,
    { content }
  );
  let payload: unknown = null;
  try {
    payload = JSON.parse(await res.text()) as unknown;
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** Event-stream status without consuming the stream (guest 404 halves). */
export async function serverAssistantEventsStatus(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/events/?after=0`,
    { headers: { cookie: sessionCookie } }
  );
  // Fetch resolves on headers; cancel the body so a 200 stream does not
  // hold the socket open.
  await res.body?.cancel().catch(() => undefined);
  return res.status;
}

/** Ask the worker to stop the active turn (204 while busy, 409 when idle). */
export async function serverAssistantCancelRaw(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/cancel/`,
    sessionCookie,
    {}
  );
  return { status: res.status };
}

/** List the caller's tool-server registry rows. */
export async function serverAssistantMcpList(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantMcpServer[]> {
  const res = await apiJson("GET", "/api/users/me/ai-assistant/mcp-servers/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] tool-server list failed with HTTP ${res.status}.`);
  if (!Array.isArray(res.payload)) throw new Error("[parity] tool-server list carried no array.");
  return res.payload.map(assistantMcpServerOf);
}

/** Register a tool server (dead URLs welcome: they prove the skipped path). */
export async function serverAssistantMcpCreate(
  sessionCookie: string,
  input: { name: string; url: string; is_enabled?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantMcpServer> {
  const res = await mutateJSON("POST", `${apiBase}/api/users/me/ai-assistant/mcp-servers/`, sessionCookie, input);
  if (res.status !== 201) throw new Error(`[parity] tool-server create failed with HTTP ${res.status}.`);
  return assistantMcpServerOf((await res.json()) as unknown);
}

/** Remove a tool-server registry row (restores the pre-scenario registry). */
export async function serverAssistantMcpDelete(
  serverId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/users/me/ai-assistant/mcp-servers/${serverId}/`,
    sessionCookie
  );
  if (res.status !== 204) throw new Error(`[parity] tool-server delete failed with HTTP ${res.status}.`);
}

/**
 * Poll a transcript until the newest row is terminal (an assistant reply
 * that finished, or an error row) or the budget runs out. The seeded stack
 * has no model backend, so keyed sends always land on the error row within
 * seconds; the poll keeps every scenario convergent on a shared stack.
 */
export async function serverAssistantPollTerminal(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  timeoutMs = 120_000,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantMessage[]> {
  const started = Date.now();
  let messages: AssistantMessage[] = [];
  for (;;) {
    messages = await serverAssistantMessages(workspaceSlug, threadId, sessionCookie, 0, 200, apiBase);
    const newest = messages[messages.length - 1];
    if (
      newest !== undefined &&
      (newest.role === "error" || (newest.role === "assistant" && newest.status === "completed"))
    ) {
      return messages;
    }
    if (Date.now() - started > timeoutMs) {
      throw new Error("[parity] transcript never reached a terminal row.");
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}

// -- Assistant-plus server reads (NEWFRONT-188, AGT-050–052/058–061/063). --

/** Connectivity-check outcome shape both key-test endpoints share. */
export interface AssistantKeyTest {
  ok: boolean;
  error_code: string | null;
  detail: string | null;
}

function assistantKeyTestOf(payload: unknown): AssistantKeyTest {
  const row = payload as Record<string, unknown>;
  return {
    ok: row["ok"] === true,
    error_code: typeof row["error_code"] === "string" ? row["error_code"] : null,
    detail: typeof row["detail"] === "string" ? row["detail"] : null,
  };
}

/** Run the provider-key connectivity check (200 with ok/error_code). */
export async function serverAssistantConfigTest(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantKeyTest> {
  const res = await mutateJSON("POST", `${apiBase}/api/users/me/ai-assistant/config/test/`, sessionCookie, {});
  if (res.status !== 200) throw new Error(`[parity] key check failed with HTTP ${res.status}.`);
  return assistantKeyTestOf((await res.json()) as unknown);
}

/** Run the speech-key connectivity check (200 with ok/error_code). */
export async function serverAssistantSttTest(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantKeyTest> {
  const res = await mutateJSON("POST", `${apiBase}/api/users/me/ai-assistant/stt-config/test/`, sessionCookie, {});
  if (res.status !== 200) throw new Error(`[parity] speech-key check failed with HTTP ${res.status}.`);
  return assistantKeyTestOf((await res.json()) as unknown);
}

/** Delete a chat thread (reserved-method proofs; the chat surface has no button). */
export async function serverAssistantThreadDelete(
  workspaceSlug: string,
  threadId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/threads/${threadId}/`,
    sessionCookie
  );
  if (res.status !== 204) throw new Error(`[parity] thread delete failed with HTTP ${res.status}.`);
}

/** Rename/toggle a tool-server registry row (registry proofs). */
export async function serverAssistantMcpPatch(
  serverId: string,
  sessionCookie: string,
  patch: { name?: string; url?: string; is_enabled?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantMcpServer> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/users/me/ai-assistant/mcp-servers/${serverId}/`,
    sessionCookie,
    patch
  );
  if (res.status !== 200) throw new Error(`[parity] tool-server patch failed with HTTP ${res.status}.`);
  return assistantMcpServerOf((await res.json()) as unknown);
}

/** Raw transcribe outcome: status plus payload (gate proofs). */
export interface AssistantTranscribeRaw {
  status: number;
  payload: unknown;
}

/**
 * POST an audio upload to the transcribe endpoint. Raw shape: callers
 * assert the gate codes (missing config, missing file, oversize) as well
 * as the happy path.
 */
export async function serverAssistantTranscribeRaw(
  sessionCookie: string,
  file: { bytes: Uint8Array; filename: string; contentType: string } | null,
  apiBase: string = apiBaseFromEnv()
): Promise<AssistantTranscribeRaw> {
  const form = new FormData();
  if (file !== null) {
    form.append("file", new Blob([file.bytes], { type: file.contentType }), file.filename);
  }
  const res = await fetch(`${apiBase}/api/users/me/ai-assistant/transcribe/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
    body: form,
  });
  let payload: unknown = null;
  try {
    payload = (await res.json()) as unknown;
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** Raw GPT-editor outcome: status plus payload (gate proofs). */
export interface WorkspaceGptRaw {
  status: number;
  payload: unknown;
}

/** POST a GPT-editor task; raw shape for the no-LLM-key refusal half. */
export async function serverWorkspaceGptRaw(
  workspaceSlug: string,
  sessionCookie: string,
  input: { prompt: string; task: string },
  apiBase: string = apiBaseFromEnv()
): Promise<WorkspaceGptRaw> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  let payload: unknown = null;
  try {
    payload = (await res.json()) as unknown;
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** Raw rephrase outcome: status only (missing-backend proofs). */
export async function serverRephraseStatus(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/rephrase-grammar/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ task: "ask_anything", text_input: "parity probe" }),
  });
  return res.status;
}

/** Whether the instance reports an LLM key (the editor-AI gate flag). */
export async function serverInstanceLlmConfigured(apiBase: string = apiBaseFromEnv()): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/instances/`);
  if (!res.ok) throw new Error(`[parity] instance read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { config?: { has_llm_configured?: unknown } };
  return payload["config"]?.["has_llm_configured"] === true;
}

// ---------------------------------------------------------------------------
// Notifications inbox foundation (NEWFRONT-198, NTF-001..006). Server reads
// and fixture writes behind the inbox specs: the parameterized list, the
// unread totals, per-item and bulk mark-read (fixture setup only — the
// UI controls stay with their owning rows), and the mention-comment wire
// format the fan-out parses. Appended; existing helpers above are untouched.
// ---------------------------------------------------------------------------

/** Query flags the inbox list endpoint accepts (mirrors the app's own params). */
export interface NotificationsListParams {
  mentioned?: boolean;
  read?: boolean;
  archived?: boolean;
  snoozed?: boolean;
  type?: string;
  perPage?: number;
  cursor?: string;
}

/** One inbox row as the list endpoint reports it. */
export interface NotificationsRow {
  id: string;
  sender: string;
  entityIdentifier: string;
  entityName: string;
  isMentioned: boolean;
  /** Whether the row's work item sits in the triage queue (NEWFRONT-199, NTF-008). */
  isInboxIssue: boolean;
  readAt: string | null;
  archivedAt: string | null;
  snoozedTill: string | null;
  createdAt: string;
  issueName: string;
  issueIdentifier: string;
  issueSequenceId: number | null;
}

/** Unread totals as the unread-count endpoint reports them. */
export interface NotificationsUnread {
  total: number;
  mentions: number;
}

function notificationsRowOf(row: unknown): NotificationsRow {
  const record = row as Record<string, unknown>;
  const data = (record["data"] as Record<string, unknown> | undefined) ?? {};
  const issue = (data["issue"] as Record<string, unknown> | undefined) ?? {};
  if (
    typeof record["id"] !== "string" ||
    typeof record["sender"] !== "string" ||
    typeof record["entity_identifier"] !== "string" ||
    typeof record["entity_name"] !== "string"
  ) {
    throw new Error("[parity] notification row carried no usable identity fields.");
  }
  return {
    id: record["id"],
    sender: record["sender"],
    entityIdentifier: record["entity_identifier"],
    entityName: record["entity_name"],
    isMentioned: record["is_mentioned_notification"] === true,
    isInboxIssue: record["is_inbox_issue"] === true,
    readAt: typeof record["read_at"] === "string" ? record["read_at"] : null,
    archivedAt: typeof record["archived_at"] === "string" ? record["archived_at"] : null,
    snoozedTill: typeof record["snoozed_till"] === "string" ? record["snoozed_till"] : null,
    createdAt: typeof record["created_at"] === "string" ? record["created_at"] : "",
    issueName: typeof issue["name"] === "string" ? issue["name"] : "",
    issueIdentifier: typeof issue["identifier"] === "string" ? issue["identifier"] : "",
    issueSequenceId: typeof issue["sequence_id"] === "number" ? issue["sequence_id"] : null,
  };
}

/**
 * Inbox list with the caller's session and query flags. Without flags this
 * mirrors the full-stream tab (mentions excluded server-side); with
 * `mentioned: true` it mirrors the mentions tab.
 */
export async function serverNotificationsList(
  workspaceSlug: string,
  sessionCookie: string,
  params: NotificationsListParams = {},
  apiBase: string = apiBaseFromEnv()
): Promise<NotificationsRow[]> {
  const query = new URLSearchParams();
  if (params.mentioned !== undefined) query.set("mentioned", String(params.mentioned));
  if (params.read !== undefined) query.set("read", String(params.read));
  if (params.archived !== undefined) query.set("archived", String(params.archived));
  if (params.snoozed !== undefined) query.set("snoozed", String(params.snoozed));
  if (params.type !== undefined) query.set("type", params.type);
  if (params.perPage !== undefined) query.set("per_page", String(params.perPage));
  if (params.cursor !== undefined) query.set("cursor", params.cursor);
  const suffix = query.size > 0 ? `?${query}` : "";
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${suffix}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notifications list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map(notificationsRowOf);
}

/** One inbox page as the list endpoint reports it (NEWFRONT-199, NTF-010). */
export interface NotificationsPage {
  rows: NotificationsRow[];
  nextPageResults: boolean;
  nextCursor: string | null;
  totalResults: number;
}

/**
 * Inbox list page with its pagination envelope. Mirrors
 * serverNotificationsList's query flags; the envelope carries the
 * next-page cursor the client's explicit control follows.
 */
export async function serverNotificationsPage(
  workspaceSlug: string,
  sessionCookie: string,
  params: NotificationsListParams = {},
  apiBase: string = apiBaseFromEnv()
): Promise<NotificationsPage> {
  const query = new URLSearchParams();
  if (params.mentioned !== undefined) query.set("mentioned", String(params.mentioned));
  if (params.read !== undefined) query.set("read", String(params.read));
  if (params.archived !== undefined) query.set("archived", String(params.archived));
  if (params.snoozed !== undefined) query.set("snoozed", String(params.snoozed));
  if (params.type !== undefined) query.set("type", params.type);
  if (params.perPage !== undefined) query.set("per_page", String(params.perPage));
  if (params.cursor !== undefined) query.set("cursor", params.cursor);
  const suffix = query.size > 0 ? `?${query}` : "";
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${suffix}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notifications page failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const rows: unknown[] = Array.isArray(payload["results"]) ? payload["results"] : [];
  return {
    rows: rows.map(notificationsRowOf),
    nextPageResults: payload["next_page_results"] === true,
    nextCursor: typeof payload["next_cursor"] === "string" ? payload["next_cursor"] : null,
    totalResults: typeof payload["total_results"] === "number" ? payload["total_results"] : rows.length,
  };
}

/** Unread totals for the session owner in this workspace. */
export async function serverNotificationsUnread(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<NotificationsUnread> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/unread/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] unread-count read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  if (
    typeof payload["total_unread_notifications_count"] !== "number" ||
    typeof payload["mention_unread_notifications_count"] !== "number"
  ) {
    throw new Error("[parity] unread-count response carried no numeric totals.");
  }
  return {
    total: payload["total_unread_notifications_count"],
    mentions: payload["mention_unread_notifications_count"],
  };
}

/** Mark one notification read (fixture setup; the UI control is NTF-018's). */
export async function serverNotificationMarkRead(
  workspaceSlug: string,
  notificationId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${notificationId}/read/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notification mark-read failed with HTTP ${res.status}.`);
}

/** Mark one notification unread (fixture setup; the UI control is NTF-018's). */
export async function serverNotificationMarkUnread(
  workspaceSlug: string,
  notificationId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${notificationId}/read/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notification mark-unread failed with HTTP ${res.status}.`);
}

/** Archive one notification (fixture setup; the UI control is NTF-019's). */
export async function serverNotificationArchive(
  workspaceSlug: string,
  notificationId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${notificationId}/archive/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notification archive failed with HTTP ${res.status}.`);
}

/** Unarchive one notification (fixture setup; the UI control is NTF-019's). */
export async function serverNotificationUnarchive(
  workspaceSlug: string,
  notificationId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${notificationId}/archive/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] notification unarchive failed with HTTP ${res.status}.`);
}

/**
 * Unsubscribe the session owner from an issue (fixture setup for origin
 * isolation: keeps an assigned fixture out of the subscribed origin).
 */
export async function serverUnsubscribeIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/subscribe/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] issue unsubscribe failed with HTTP ${res.status}.`);
}

/** Mark the scope read in bulk (fixture setup; the UI control is NTF-014's). */
export async function serverNotificationsMarkAllRead(
  workspaceSlug: string,
  sessionCookie: string,
  scope: Record<string, unknown> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/mark-all-read/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(scope),
  });
  if (!res.ok) throw new Error(`[parity] mark-all-read failed with HTTP ${res.status}.`);
}

/**
 * Comment HTML that names `userId`: the mention tag the comment fan-out
 * parses, plus `bodyText` so the row stays findable. Attribute names follow
 * the backend parser contract (mention-component with entity_name and
 * entity_identifier), the same wire format the stored-comment assertions
 * in the mention specs already pin.
 */
export function serverMentionHtml(userId: string, displayName: string, bodyText: string): string {
  const safeName = displayName.replace(/[<>&]/g, "");
  const safeBody = bodyText.replace(/[<>&]/g, "");
  return (
    `<p><mention-component entity_identifier="${userId}" entity_name="user_mention">` +
    `@${safeName}</mention-component> ${safeBody}</p>`
  );
}

/**
 * Mark the session owner fully onboarded with the tour completed, so a
 * freshly provisioned user can enter app pages directly instead of being
 * funneled into onboarding. Same end state as the onboarding specs'
 * markOnboarded, driven from a session cookie.
 */
export async function serverOnboardSession(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const profile = await mutateJSON("PATCH", `${apiBase}/api/users/me/profile/`, sessionCookie, {
    onboarding_step: {
      profile_complete: true,
      workspace_create: true,
      workspace_join: true,
      workspace_invite: true,
    },
  });
  if (!profile.ok) throw new Error(`[parity] onboard profile update failed with HTTP ${profile.status}.`);
  const onboard = await mutateJSON("PATCH", `${apiBase}/api/users/me/onboard/`, sessionCookie, {
    is_onboarded: true,
  });
  if (!onboard.ok) throw new Error(`[parity] onboard marker update failed with HTTP ${onboard.status}.`);
  await serverSetTourCompleted(sessionCookie, true, apiBase);
}

// --- Desktop agent-runtime web-observable sides (NEWFRONT-207, DESK-001–010,
// --- DESK-026). Appended; existing helpers above are untouched per the shared
// --- harness contract. Session-cookie style like the serverDesktopRuntime*
// --- block (NEWFRONT-182): every helper takes a signed-in session cookie plus
// --- the ids it needs. Refusal halves (*Status / raw outcomes) resolve with
// --- the outcome so scenarios can assert instead of throwing.

/** One executor option as project detail serializes it. */
export interface DeskRuntimeExecutorOption {
  kind: string;
  available: boolean;
  reasonCode: string;
}

function deskRuntimeExecutorOptionOf(raw: unknown): DeskRuntimeExecutorOption {
  const record = raw as { kind?: unknown; available?: unknown; reason_code?: unknown };
  if (
    typeof record.kind !== "string" ||
    typeof record.available !== "boolean" ||
    typeof record.reason_code !== "string"
  ) {
    throw new Error("[parity] executor option missed kind/available/reason_code.");
  }
  return { kind: record.kind, available: record.available, reasonCode: record.reason_code };
}

/** Executor options for a project (the managed-runner reason-code surface). */
export async function serverDeskRuntimeProjectExecutorOptions(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeExecutorOption[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { agent_executor_options?: unknown };
  const rows: unknown[] = Array.isArray(payload.agent_executor_options) ? payload.agent_executor_options : [];
  return rows.map(deskRuntimeExecutorOptionOf);
}

/** Raw outcome of pinning an issue's executor (refusal halves resolve). */
export interface DeskRuntimePinOutcome {
  status: number;
  body: string;
}

/**
 * Pin an issue's executor, resolving with the raw outcome. A pin the server
 * refuses (e.g. the desktop target from a web session) resolves with its
 * 4xx status instead of throwing.
 */
export async function serverDeskRuntimePinIssueExecutorStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  executor: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimePinOutcome> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    { agent_executor: executor }
  );
  return { status: res.status, body: await res.text() };
}

/** Current executor selection of an issue (null when inheriting the default). */
export async function serverDeskRuntimeIssueExecutor(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] issue read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { agent_executor?: unknown };
  if (payload.agent_executor === null || payload.agent_executor === undefined) return null;
  if (typeof payload.agent_executor !== "string") throw new Error("[parity] issue row carried no agent executor.");
  return payload.agent_executor;
}

/** Raw outcome of a chat lifecycle call (resolves, never throws). */
export interface DeskRuntimeChatAction {
  status: number;
  body: string;
}

async function deskRuntimeChatActionOf(res: Response): Promise<DeskRuntimeChatAction> {
  return { status: res.status, body: await res.text() };
}

/** Warm a chat session through the app's own endpoint. */
export async function serverDeskRuntimeWarmChatSession(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeChatAction> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/warm/`, sessionCookie, {});
  return deskRuntimeChatActionOf(res);
}

/** Cancel a chat session's active turn through the app's own endpoint. */
export async function serverDeskRuntimeCancelChatSession(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeChatAction> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/cancel/`, sessionCookie, {});
  return deskRuntimeChatActionOf(res);
}

/** Close a chat session through the app's own endpoint. */
export async function serverDeskRuntimeCloseChatSession(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeChatAction> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/sessions/${sessionId}/close/`, sessionCookie, {});
  return deskRuntimeChatActionOf(res);
}

/** Minimal session detail the cancel/close halves assert on. */
export interface DeskRuntimeChatSessionDetail {
  id: string;
  status: string;
  closeRequested: boolean;
}

/** One chat session through the app's own endpoint. */
export async function serverDeskRuntimeChatSessionDetail(
  sessionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeChatSessionDetail> {
  const res = await fetchTolerant(`${apiBase}/api/runners/chat/sessions/${sessionId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] chat-session read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; status?: unknown; close_requested?: unknown };
  if (typeof record.id !== "string" || typeof record.status !== "string") {
    throw new Error("[parity] chat-session payload carried no id/status.");
  }
  return { id: record.id, status: record.status, closeRequested: record.close_requested === true };
}

/** A decided chat approval as the decide endpoint returns it. */
export interface DeskRuntimeChatDecision {
  id: string;
  status: string;
  decisionSource: string;
}

/**
 * Settle a session's active turn exactly the way the daemon's
 * message-complete upstream would (status the active message completed,
 * clear the turn), so the next send is admitted. The scratch stack runs no
 * live daemon, so without this every session accepts exactly one turn;
 * this stands in for the daemon write-back the way the approval plant
 * does. It invents no transcript content — only the turn bookkeeping.
 */
export async function serverDeskRuntimeSettleChatTurn(sessionId: string): Promise<void> {
  const out = await desktopRuntimeShell(
    `from django.db import transaction\n` +
      `from pi_dash.runner.models import AgentChatSession\n` +
      `from pi_dash.runner.services import chat as chat_service\n` +
      `with transaction.atomic():\n` +
      `    session = AgentChatSession.objects.select_for_update().get(pk=${JSON.stringify(sessionId)})\n` +
      `    chat_service.complete_active_turn_locked(session)\n` +
      `print("PARITY_CHAT_SETTLED")\n`
  );
  if (!out.includes("PARITY_CHAT_SETTLED")) throw new Error("[parity] chat-turn settle produced no marker.");
}

/** Decide a pending chat approval through the user-facing queue endpoint. */
export async function serverDeskRuntimeDecideChatApproval(
  approvalId: string,
  decision: "accept" | "decline",
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<DeskRuntimeChatDecision> {
  const res = await mutateJSON("POST", `${apiBase}/api/runners/chat/approvals/${approvalId}/decide/`, sessionCookie, {
    decision,
  });
  if (res.status !== 200) throw new Error(`[parity] chat-approval decide failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; status?: unknown; decision_source?: unknown };
  if (
    typeof record.id !== "string" ||
    typeof record.status !== "string" ||
    typeof record.decision_source !== "string"
  ) {
    throw new Error("[parity] chat-decision payload missed id/status/decision_source.");
  }
  return { id: record.id, status: record.status, decisionSource: record.decision_source };
}

// --- Notifications snooze + email preferences (NEWFRONT-201, NTF-020..022,
// --- NTF-024..025). Snooze fixture writes, email-preference reads/writes,
// --- and email-log counts for the delivery-gating proof. Appended; existing
// --- helpers above are untouched per the shared harness contract.

/**
 * Snooze or unsnooze one notification (fixture setup; the UI control is
 * NTF-020's). A null resume clears the snooze, matching the app's own
 * unsnooze write (the view defaults a missing timestamp to null).
 */
export async function serverNotificationSnooze(
  workspaceSlug: string,
  notificationId: string,
  sessionCookie: string,
  snoozedTill: string | null,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const body: Record<string, string | null> =
    snoozedTill === null ? { snoozed_till: null } : { snoozed_till: snoozedTill };
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/users/notifications/${notificationId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`[parity] notification snooze failed with HTTP ${res.status}.`);
}

/** Email-preference toggles as the server reports them. */
export interface EmailNotificationPreferences {
  property_change: boolean;
  state_change: boolean;
  issue_completed: boolean;
  comment: boolean;
  mention: boolean;
}

function emailPreferencesOf(payload: unknown): EmailNotificationPreferences {
  const record = payload as Record<string, unknown>;
  for (const key of ["property_change", "state_change", "issue_completed", "comment", "mention"]) {
    if (typeof record[key] !== "boolean") {
      throw new Error("[parity] email-preference payload carried a non-boolean toggle.");
    }
  }
  return {
    property_change: record["property_change"] as boolean,
    state_change: record["state_change"] as boolean,
    issue_completed: record["issue_completed"] as boolean,
    comment: record["comment"] as boolean,
    mention: record["mention"] as boolean,
  };
}

/** Current email-preference toggles for the session owner. */
export async function serverEmailPreferences(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<EmailNotificationPreferences> {
  const res = await fetch(`${apiBase}/api/users/me/notification-preferences/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] email-preference read failed with HTTP ${res.status}.`);
  return emailPreferencesOf(await res.json());
}

/** Save email-preference toggles for the session owner (fixture setup). */
export async function serverUpdateEmailPreferences(
  sessionCookie: string,
  patch: Partial<EmailNotificationPreferences>,
  apiBase: string = apiBaseFromEnv()
): Promise<EmailNotificationPreferences> {
  const res = await fetch(`${apiBase}/api/users/me/notification-preferences/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`[parity] email-preference save failed with HTTP ${res.status}.`);
  return emailPreferencesOf(await res.json());
}

/**
 * Run a Django shell snippet inside a chosen api container; resolves with
 * stdout. Same shape as apiShell, with the container name overridable for
 * runs on a renamed stack (PARITY_API_CONTAINER); apiShell itself stays
 * pinned to the default stack per the shared harness contract.
 */
export async function apiShellOn(container: string, python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", container, "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/**
 * Email-log rows queued for `receiverEmail` on one work item. The fan-out
 * writes these only when the receiver's per-topic preference allows, while
 * the in-app rows always land — so the count is the delivery-gating signal
 * NTF-025 asserts on.
 */
export async function serverEmailLogCount(receiverEmail: string, entityIdentifier: string): Promise<number> {
  const container = (process.env["PARITY_API_CONTAINER"] ?? "parity19-api").trim() || "parity19-api";
  const out = await apiShellOn(
    container,
    `from pi_dash.db.models import EmailNotificationLog, User\n` +
      `user = User.objects.get(email=${JSON.stringify(receiverEmail)})\n` +
      `print("PARITY_EMAIL_LOGS:" + str(EmailNotificationLog.objects.filter(receiver=user, entity_identifier=${JSON.stringify(entityIdentifier)}).count()))\n`
  );
  const count = /^PARITY_EMAIL_LOGS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (!/^\d+$/.test(count)) throw new Error("[parity] email-log count produced no numeric row.");
  return Number(count);
}

// --- Archives: modules/cycles fixtures (NEWFRONT-225, ARCH-020..025). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. Fixture writes (patch/archive/restore) plus server-state reads
// for the archived-modules oracle scenarios. Restore uses the live
// frontend path DELETE .../modules/{id}/archive/ (probed 204 on the seeded
// stack; the Notes-flagged .../archived-modules/{id}/unarchive/ 404s on
// /api and needs token auth on /api/v1, so it is not the app path).

/** One archived module as the archived-modules list reports it. */
export interface ArchivesModuleRow {
  id: string;
  name: string;
  status: string;
  archivedAt: string | null;
}

function archivesModuleRowOf(row: Record<string, unknown>): ArchivesModuleRow {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string") {
    throw new Error("[parity] archived-module row carried no string id/name.");
  }
  const archivedAt = row["archived_at"];
  return {
    id: row["id"],
    name: row["name"],
    status: typeof row["status"] === "string" ? row["status"] : "",
    archivedAt: typeof archivedAt === "string" ? archivedAt : null,
  };
}

/** Patch a module (fixture setup: status, lead, dates). Unknown keys are the server's to reject. */
export async function archivesPatchModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  patch: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    sessionCookie,
    patch
  );
  if (!res.ok) throw new Error(`[parity] module patch failed with HTTP ${res.status}.`);
}

/** Archive one module; resolves with the stored archived_at stamp. */
export async function archivesArchiveModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/archive/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] module archive failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["archived_at"] !== "string") throw new Error("[parity] module archive carried no archived_at.");
  return rec["archived_at"];
}

/** Restore one archived module through the live frontend path. */
export async function archivesRestoreModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/archive/`,
    sessionCookie
  );
  if (res.status !== 204 && res.status !== 200) {
    throw new Error(`[parity] module restore failed with HTTP ${res.status}.`);
  }
}

/** Archived modules of a project, in API order. */
export async function archivesArchivedModules(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ArchivesModuleRow[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-modules/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-modules read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => archivesModuleRowOf(row as Record<string, unknown>));
}

/** One archived module's detail record (what the peek panel fetches). */
export async function archivesArchivedModuleDetail(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-modules/${moduleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] archived-module detail read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

/**
 * Whether a module currently reads as archived. The live detail read 404s
 * for archived modules (probed), so a 404 means archived; a 200 resolves
 * from the record's own stamp. Any other status throws.
 */
export async function archivesModuleIsArchived(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return true;
  if (!res.ok) throw new Error(`[parity] module detail read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return typeof rec["archived_at"] === "string" && rec["archived_at"] !== "";
}

/** Archive one cycle; the cycle must already be past its end date. */
export async function archivesArchiveCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] cycle archive failed with HTTP ${res.status}.`);
}

/** Archived cycles of a project: id plus name. */
export async function archivesArchivedCycles(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-cycles/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-cycles read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string") {
      throw new Error("[parity] archived-cycle row carried no string id/name.");
    }
    return { id: rec["id"], name: rec["name"] };
  });
}

// --- Archived work-items list oracles (NEWFRONT-222, ARCH-001..007).
// --- Appended; existing helpers above are untouched per the shared
// --- contract.
/** View flags gating the archives tab strip, as the server reports them. */
export async function serverArchivesProjectFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ cycleView: boolean; moduleView: boolean }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project flags read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  if (typeof record["cycle_view"] !== "boolean" || typeof record["module_view"] !== "boolean")
    throw new Error("[parity] project carried no boolean cycle_view/module_view.");
  return { cycleView: record["cycle_view"], moduleView: record["module_view"] };
}

// --- Views area server helpers (NEWFRONT-42, VIEW-*). Appended; existing
// --- helpers above are untouched per the shared harness contract. Two
// --- inventory corrections are baked in: the UI favorites views through
// --- the generic user-favorites collection (entity_type "view"), never
// --- through the legacy user-favorite-views service methods (no callers);
// --- and a view detail's issues resolve through the issues list with the
// --- view's stored query, never through the uncalled
// --- views/{id}/issues/ service method (no backend route).

/** Access flag as the server stores it: 0 private, 1 public. */
export const VIEW_ACCESS = { PRIVATE: 0, PUBLIC: 1 } as const;

/** A saved project view as the retrieve endpoint reports it. */
export interface ParityProjectViewDetail {
  id: string;
  name: string;
  description: string;
  access: number;
  owned_by: string | null;
  is_favorite: boolean;
  is_locked: boolean;
  logo_props: unknown;
  rich_filters: unknown;
  display_filters: unknown;
  display_properties: unknown;
}

function projectViewDetailOf(row: Record<string, unknown>): ParityProjectViewDetail {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string")
    throw new Error("[parity] view row carried no string id/name.");
  const owned = row["owned_by"];
  return {
    id: row["id"],
    name: row["name"],
    description: typeof row["description"] === "string" ? row["description"] : "",
    access: typeof row["access"] === "number" ? row["access"] : VIEW_ACCESS.PUBLIC,
    owned_by: typeof owned === "string" ? owned : null,
    is_favorite: row["is_favorite"] === true,
    is_locked: row["is_locked"] === true,
    logo_props: row["logo_props"] ?? null,
    rich_filters: row["rich_filters"] ?? null,
    display_filters: row["display_filters"] ?? null,
    display_properties: row["display_properties"] ?? null,
  };
}

/** One saved project view by id, as the server reports it. */
export async function serverProjectViewDetail(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectViewDetail> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] view detail read failed with HTTP ${res.status}.`);
  return projectViewDetailOf((await res.json()) as Record<string, unknown>);
}

/** Raw view-detail status (missing/forbidden proofs read the code, not the row). */
export async function serverProjectViewStatus(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`, {
    headers: { cookie: sessionCookie },
  });
  return res.status;
}

/** Create a saved project view with a full payload; returns its id. */
export async function createProjectViewFull(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`,
    sessionCookie,
    body
  );
  if (!res.ok) throw new Error(`[parity] view create failed with HTTP ${res.status}.`);
  const view = (await res.json()) as { id?: unknown };
  if (typeof view.id !== "string") throw new Error("[parity] view create returned no id.");
  return view.id;
}

/** Raw view-create outcome: status plus payload (validation proofs). */
export async function serverCreateProjectViewRaw(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/`,
    sessionCookie,
    body
  );
  let payload: unknown = null;
  try {
    payload = (await res.json()) as unknown;
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** Patch a saved project view (owner-only server-side). */
export async function patchProjectView(
  workspaceSlug: string,
  projectId: string,
  viewId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/views/${viewId}/`,
    sessionCookie,
    body
  );
  if (!res.ok) throw new Error(`[parity] view patch failed with HTTP ${res.status}.`);
}

/**
 * Plant fixture state the OSS API cannot write: access and is_locked are
 * serializer read-only (VIEW-019/VIEW-025), so the lock/private render
 * paths are proven by flipping the row in the container and reading it
 * back through the public GET. Callers delete the view after.
 */
export async function serverPlantViewFlags(
  viewId: string,
  flags: { access?: number; is_locked?: boolean }
): Promise<void> {
  const sets: string[] = [];
  if (flags.access !== undefined) sets.push(`access=${JSON.stringify(flags.access)}`);
  if (flags.is_locked !== undefined) sets.push(`is_locked=${flags.is_locked ? "True" : "False"}`);
  if (sets.length === 0) return;
  const out = await apiShell(
    `from pi_dash.db.models import IssueView\n` +
      `IssueView.objects.filter(id=${JSON.stringify(viewId)}).update(${sets.join(", ")})\n` +
      `print("PARITY_VIEW_FLAGS_OK")\n`
  );
  if (!out.includes("PARITY_VIEW_FLAGS_OK")) throw new Error("[parity] view flag plant produced no marker.");
}

/** A custom workspace view as the retrieve endpoint reports it. */
export interface ParityWorkspaceViewDetail {
  id: string;
  name: string;
  description: string;
  access: number;
  owned_by: string | null;
  is_locked: boolean;
  rich_filters: unknown;
  display_filters: unknown;
  display_properties: unknown;
}

function workspaceViewDetailOf(row: Record<string, unknown>): ParityWorkspaceViewDetail {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string")
    throw new Error("[parity] workspace view row carried no string id/name.");
  const owned = row["owned_by"];
  return {
    id: row["id"],
    name: row["name"],
    description: typeof row["description"] === "string" ? row["description"] : "",
    access: typeof row["access"] === "number" ? row["access"] : VIEW_ACCESS.PUBLIC,
    owned_by: typeof owned === "string" ? owned : null,
    is_locked: row["is_locked"] === true,
    rich_filters: row["rich_filters"] ?? null,
    display_filters: row["display_filters"] ?? null,
    display_properties: row["display_properties"] ?? null,
  };
}

/** Custom workspace views as the server reports them (defaults are static). */
export async function serverWorkspaceViews(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceViewDetail[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/views/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace views read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => workspaceViewDetailOf(row as Record<string, unknown>));
}

/** One custom workspace view by id, as the server reports it. */
export async function serverWorkspaceViewDetail(
  workspaceSlug: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceViewDetail> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/views/${viewId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace view read failed with HTTP ${res.status}.`);
  return workspaceViewDetailOf((await res.json()) as Record<string, unknown>);
}

/** Create a custom workspace view; returns its id (callers delete it after). */
export async function createWorkspaceViewFull(
  workspaceSlug: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/views/`, sessionCookie, body);
  if (!res.ok) throw new Error(`[parity] workspace view create failed with HTTP ${res.status}.`);
  const view = (await res.json()) as { id?: unknown };
  if (typeof view.id !== "string") throw new Error("[parity] workspace view create returned no id.");
  return view.id;
}

/** Patch a custom workspace view (owner-only server-side). */
export async function patchWorkspaceView(
  workspaceSlug: string,
  viewId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/views/${viewId}/`,
    sessionCookie,
    body
  );
  if (!res.ok) throw new Error(`[parity] workspace view patch failed with HTTP ${res.status}.`);
}

/** Delete a custom workspace view (keeps scenarios from leaking views). */
export async function deleteWorkspaceView(
  workspaceSlug: string,
  viewId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("DELETE", `${apiBase}/api/workspaces/${workspaceSlug}/views/${viewId}/`, sessionCookie);
  if (!res.ok && res.status !== 204) throw new Error(`[parity] workspace view delete failed with HTTP ${res.status}.`);
}

/** A user-favorite row as the server reports it. */
export interface ParityUserFavorite {
  id: string;
  entity_type: string;
  entity_identifier: string;
  project_id: string | null;
}

/** The session user's favorites in a workspace (views star through these). */
export async function serverUserFavorites(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityUserFavorite[]> {
  // NOTE: `?all=true` like the app itself — the bare list omits view rows.
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/user-favorites/?all=true`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] favorites read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const fav = row as { id?: unknown; entity_type?: unknown; entity_identifier?: unknown; project_id?: unknown };
    if (typeof fav.id !== "string" || typeof fav.entity_identifier !== "string")
      throw new Error("[parity] favorite row carried no id/entity.");
    return {
      id: fav.id,
      entity_type: typeof fav.entity_type === "string" ? fav.entity_type : "",
      entity_identifier: fav.entity_identifier,
      project_id: typeof fav.project_id === "string" ? fav.project_id : null,
    };
  });
}

/** Delete one user-favorite row (favorite-toggle cleanup). */
export async function deleteUserFavorite(
  workspaceSlug: string,
  favoriteId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/user-favorites/${favoriteId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204) throw new Error(`[parity] favorite delete failed with HTTP ${res.status}.`);
}

/** Display name of the session user (chips show real names, never "You"). */
export async function serverDisplayName(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetchShared(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] users-me read failed with HTTP ${res.status}.`);
  const me = (await res.json()) as { display_name?: unknown };
  if (typeof me.display_name !== "string" || me.display_name === "")
    throw new Error("[parity] users-me carried no display name.");
  return me.display_name;
}

// --- Archived-cycles oracle helpers (NEWFRONT-224, ARCH-014–019). Appended;
// --- existing helpers above are untouched per the shared harness contract.

/** One archived cycle row as the archived-cycles endpoints return it. */
export interface ParityServerArchivedCycle {
  id: string;
  name: string;
  archivedAt: string | null;
}

/**
 * Archive a cycle through the API (setup for UI scenarios, never the
 * assertion). The server only archives cycles whose end date is past, so
 * fixtures backdate first (see serverPatchCycle); throws otherwise.
 */
export async function serverArchiveCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] cycle archive failed with HTTP ${res.status}.`);
}

/**
 * Restore an archived cycle through the API (teardown for UI scenarios).
 * Targets the frontend-called unarchive path (DELETE on the cycle archive
 * endpoint), which the NEWFRONT-224 stack probe confirmed live (204; the
 * inventory-claimed archived-cycles unarchive path 404s); tolerates 404 so
 * teardown never fails a scenario whose UI already restored the cycle.
 */
export async function serverRestoreCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/archive/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 204 && res.status !== 404)
    throw new Error(`[parity] cycle restore failed with HTTP ${res.status}.`);
}

/** Archived cycles of a project, in API order. */
export async function serverArchivedCycles(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerArchivedCycle[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-cycles/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-cycles read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown; archived_at?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string") {
      throw new Error("[parity] archived-cycle row carried no string id and name.");
    }
    return {
      id: record.id,
      name: record.name,
      archivedAt: typeof record.archived_at === "string" ? record.archived_at : null,
    };
  });
}

/** One archived cycle's detail (the peek body source). */
export async function serverArchivedCycleDetail(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerArchivedCycle> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-cycles/${cycleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] archived-cycle detail read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { id?: unknown; name?: unknown; archived_at?: unknown };
  if (typeof record.id !== "string" || typeof record.name !== "string") {
    throw new Error("[parity] archived-cycle detail carried no string id and name.");
  }
  return {
    id: record.id,
    name: record.name,
    archivedAt: typeof record.archived_at === "string" ? record.archived_at : null,
  };
}

// --- Archives cross-cutting (NEWFRONT-226, ARCH-026–032). Appended; the
// --- helpers above are untouched per the shared contract.
// Fixture archiving reuses the sibling helpers archivesArchiveCycle,
// archivesArchiveModule, and archivesPatchModule (same endpoints).
/**
 * archived_at of one project (null when live). Reads the projects/details
 * collection: the single-project read 404s once archived.
 */
export async function archivesProjectArchivedAt(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/details/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] projects/details read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const match = rows.find((row) => (row as Record<string, unknown>)["id"] === projectId) as
    | Record<string, unknown>
    | undefined;
  if (match === undefined) throw new Error("[parity] project missing from the details collection.");
  return typeof match["archived_at"] === "string" ? match["archived_at"] : null;
}

/** archived_at of one archived cycle (single-cycle GET 404s once archived). */
export async function archivesCycleArchivedAt(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-cycles/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-cycles read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const match = rows.find((row) => (row as Record<string, unknown>)["id"] === cycleId) as
    | Record<string, unknown>
    | undefined;
  if (match === undefined) throw new Error("[parity] cycle missing from the archived-cycles collection.");
  return typeof match["archived_at"] === "string" ? match["archived_at"] : null;
}

/** HTTP status of one archived-list read (permission proofs for ARCH-028). */
export async function archivesArchivedListStatus(
  workspaceSlug: string,
  projectId: string,
  list: "issues" | "cycles" | "modules",
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-${list}/`, {
    headers: { cookie: sessionCookie },
  });
  return res.status;
}

/** HTTP status of one archived-issue retrieve (permission proofs for ARCH-028). */
export async function archivesArchivedIssueStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/archive/`,
    { headers: { cookie: sessionCookie } }
  );
  return res.status;
}

/** archived_at of one archived module (single-module GET 404s once archived). */
export async function archivesModuleArchivedAt(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-modules/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] archived-modules read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const match = rows.find((row) => (row as Record<string, unknown>)["id"] === moduleId) as
    | Record<string, unknown>
    | undefined;
  if (match === undefined) throw new Error("[parity] module missing from the archived-modules collection.");
  return typeof match["archived_at"] === "string" ? match["archived_at"] : null;
}

// --- Cycles archive/menu/transfer acceptance (NEWFRONT-252, CYC-025-031
// --- plus CYC-046). Appended; the helpers above are untouched per the
// --- shared contract. Favorite setup/teardown for UI scenarios; the row
// --- marker makes the same calls.

/**
 * Favorite a cycle through the API. POSTs the cycle id to the
 * user-favorite-cycles collection (throws naming the failure).
 */
export async function cyclesFavoriteCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-favorite-cycles/`,
    sessionCookie,
    { cycle: cycleId }
  );
  if (!res.ok) throw new Error(`[parity] cycle favorite failed with HTTP ${res.status}.`);
}

/**
 * Unfavorite a cycle through the API. Tolerates 404 so teardown never
 * fails a scenario whose UI already removed the mark.
 */
export async function cyclesUnfavoriteCycle(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-favorite-cycles/${cycleId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 404) throw new Error(`[parity] cycle unfavorite failed with HTTP ${res.status}.`);
}
