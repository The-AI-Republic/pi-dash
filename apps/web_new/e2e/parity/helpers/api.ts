// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", "parity19-api", "python", "manage.py", "shell", "-c", python],
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

/** Names of the project's issues as the server reports them, in API order. */
export async function serverIssueNames(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
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
