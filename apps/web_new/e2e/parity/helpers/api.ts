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

// --- NEWFRONT-113 (rules): comment server-state helpers. Appended
// additively; existing helpers above are untouched.

/** Comment fields the rules specs assert on. */
export interface RulesServerComment {
  id: string;
  access: string;
  labels: string[];
  comment_html: string;
  is_synced: boolean;
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
  if (res.status !== 201 || !body || typeof body.id !== "string")
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
  if (res.status !== 201 || !body || typeof body.id !== "string")
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
