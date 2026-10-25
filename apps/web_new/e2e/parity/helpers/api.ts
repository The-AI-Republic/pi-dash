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
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import { parityApiUrl, parityOracleUrl, resolveParityEnv } from "./parity-env";

const execFileAsync = promisify(execFile);

function apiBaseFromEnv(): string {
  const raw = parityApiUrl(process.env).trim().replace(/\/+$/, "");
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
  // Slotted stacks rename the redis container (NEWFRONT-246); the shared
  // parity19 default is unchanged.
  const container = process.env["PARITY_REDIS_CONTAINER"] ?? "parity19-redis";
  await execFileAsync("docker", ["exec", "-i", container, "redis-cli", "SET", `magic_${email}`, payload, "EX", "600"], {
    timeout: 60_000,
  });
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
  const file = process.env["PARITY_SEED_FILE"] || defaultSeedFile();
  if (!file)
    throw new Error("[parity] PARITY_SEED_FILE is not set; run the stack seed step first (see stack/README.md).");
  return JSON.parse(readFileSync(file, "utf8")) as ParitySeedFacts;
}

// Namespaced stacks (NEWFRONT-132) seed into .seed-<ns>.json, so the
// suite finds its facts from PARITY_NS alone. The default stack keeps
// requiring an explicit PARITY_SEED_FILE, exactly as before.
function defaultSeedFile(): string | null {
  const resolved = resolveParityEnv(process.env);
  if (resolved.namespace === null) return null;
  return join(dirname(fileURLToPath(import.meta.url)), "..", resolved.seedFileName);
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
  const raw = parityOracleUrl(process.env);
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

/** Archive a project; throws unless the server accepts. */
export async function serverArchiveProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archive/`,
    sessionCookie,
    {}
  );
  if (res.status !== 200) throw new Error(`[parity] project archive failed with HTTP ${res.status}.`);
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
  entity_identifier?: string;
  entity_data?: { name?: string; id?: string } | null;
}

/** Record an issue visit the way opening its detail page does.
 *
 * The server logs the visit on issue-detail GET (and a project visit
 * on the project GET) through a celery task, so this polls the recents
 * feed until the visit lands instead of sleeping. App-agnostic: the
 * feed reads server visits no matter which client recorded them.
 */
export async function serverVisitIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await serverIssueDetail(workspaceSlug, projectId, issueId, sessionCookie, apiBase);
  const deadline = Date.now() + 60_000;
  for (;;) {
    const rows = await serverRecents(workspaceSlug, sessionCookie, "issue", apiBase);
    if (rows.some((row) => row.entity_identifier === issueId)) return;
    if (Date.now() > deadline) throw new Error(`[parity] visit for issue ${issueId} never landed in recents.`);
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
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
  // The endpoint lazy-creates the default rows on GET and answers 400 on a
  // PATCH to a missing row, so initialize before patching. Convergent: the
  // PATCH below applies the same delta whether the rows pre-existed or not.
  await serverWidgets(workspaceSlug, sessionCookie, apiBase);
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
  system: "points" | "time" | "categories" = "points",
  apiBase: string = apiBaseFromEnv(),
  lastUsed: boolean = true
): Promise<{ id: string; points: ParityServerEstimatePoint[] }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`,
    sessionCookie,
    {
      estimate: { name, type: system, last_used: lastUsed },
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
    descriptionHtml: strOrNull("description_html"),
    parentId: strOrNull("parent_id"),
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
): Promise<{
  name: string;
  startDate: string | null;
  endDate: string | null;
  snapshotEmpty: boolean;
  description: string | null;
}> {
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
    // NEWFRONT-250 (CYC-014): creation carries an optional description.
    description:
      typeof rec["description"] === "string" && (rec["description"] as string) !== ""
        ? (rec["description"] as string)
        : null,
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
  descriptionHtml: string | null;
  parentId: string | null;
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
  const raw = parityOracleUrl(process.env);
  try {
    return new URL(raw).hostname;
  } catch {
    return "localhost";
  }
}

/**
 * Cookies for `context.addCookies` so a browser acts as this user with no UI
 * login. Callers pass either a signed-in session or the bare cookie string a
 * harness hands back (`schedulerHarness().ownerSession`); a session with no
 * cookie header yields no cookies rather than throwing.
 */
export function browserSessionCookies(session: AuthedSession | string): BrowserSessionCookie[] {
  const domain = oracleHost();
  const header = typeof session === "string" ? session : session.cookie;
  if (typeof header !== "string" || header.trim() === "") return [];
  const jar = jarFromSetCookies(header.split(";").map((p) => p.trim()));
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

// --- Draft oracle helpers (NEWFRONT-32, DRAFT-001–027). Appended; existing
// --- helpers above are untouched per the shared harness contract. The
// --- Status variants never throw on refusal codes: a 403/404/400 is the
// --- asserted behavior on permission and validation paths.

/** A workspace draft record as the list/create endpoints report it. */
export interface ParityServerDraft {
  id: string;
  name: string;
  projectId: string | null;
  stateId: string | null;
  priority: string | null;
  estimatePoint: string | null;
  startDate: string | null;
  targetDate: string | null;
  cycleId: string | null;
  moduleIds: string[];
  labelIds: string[];
  assigneeIds: string[];
  descriptionHtml: string | null;
  createdAt: string;
  typeId: string | null;
}

function draftOf(row: Record<string, unknown>): ParityServerDraft {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string")
    throw new Error("[parity] draft row carried no string id/name.");
  const idsOf = (value: unknown): string[] =>
    Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
  const strOrNull = (value: unknown): string | null => (typeof value === "string" ? value : null);
  return {
    id: row["id"],
    name: row["name"],
    projectId: strOrNull(row["project_id"] ?? row["project"]),
    stateId: strOrNull(row["state_id"] ?? row["state"]),
    priority: strOrNull(row["priority"]),
    estimatePoint: strOrNull(row["estimate_point"]),
    startDate: strOrNull(row["start_date"]),
    targetDate: strOrNull(row["target_date"]),
    cycleId: strOrNull(row["cycle_id"] ?? row["cycle"]),
    moduleIds: idsOf(row["module_ids"]),
    labelIds: idsOf(row["label_ids"]),
    assigneeIds: idsOf(row["assignee_ids"]),
    descriptionHtml: strOrNull(row["description_html"]),
    createdAt: typeof row["created_at"] === "string" ? row["created_at"] : "",
    typeId: strOrNull(row["type_id"] ?? row["type"]),
  };
}

/** Fields a draft create/update accepts; the server validates project scoping and date order. */
export interface ParityDraftWrite {
  name?: string;
  description_html?: string;
  project_id?: string | null;
  state_id?: string | null;
  priority?: string | null;
  estimate_point?: string | null;
  start_date?: string | null;
  target_date?: string | null;
  cycle_id?: string | null;
  module_ids?: string[];
  label_ids?: string[];
  assignee_ids?: string[];
  parent_id?: string | null;
  type_id?: string | null;
}

/** Create a workspace draft; resolves with the stored record (HTTP 201). */
export async function serverCreateDraft(
  workspaceSlug: string,
  sessionCookie: string,
  body: ParityDraftWrite,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerDraft> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/`, sessionCookie, body);
  if (res.status !== 201)
    throw new Error(`[parity] draft create failed with HTTP ${res.status}: ${(await res.text()).slice(0, 200)}.`);
  return draftOf((await res.json()) as Record<string, unknown>);
}

/** Create a workspace draft; resolves with the raw status for refusal-path assertions. */
export async function serverCreateDraftStatus(
  workspaceSlug: string,
  sessionCookie: string,
  body: ParityDraftWrite,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/`, sessionCookie, body);
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}

/** Read one workspace draft in full (HTTP 200); throws otherwise. */
export async function serverDraftRecord(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerDraft> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] draft read failed with HTTP ${res.status}.`);
  return draftOf((await res.json()) as Record<string, unknown>);
}

/** Fetch one workspace draft; resolves with the raw status for refusal-path assertions. */
export async function serverFetchDraftStatus(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`, {
    headers: { cookie: sessionCookie },
  });
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}

/** Patch a workspace draft; the server answers 204 with no body on success. */
export async function serverPatchDraft(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  patch: ParityDraftWrite,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`,
    sessionCookie,
    patch
  );
  if (res.status !== 204)
    throw new Error(`[parity] draft patch failed with HTTP ${res.status}: ${(await res.text()).slice(0, 200)}.`);
}

/** Patch a workspace draft; resolves with the raw status for refusal-path assertions. */
export async function serverPatchDraftStatus(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  patch: ParityDraftWrite,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`,
    sessionCookie,
    patch
  );
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}

/** Delete a workspace draft; resolves with the raw status for refusal-path assertions. */
export async function serverDeleteDraftStatus(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${draftId}/`,
    sessionCookie
  );
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}

/**
 * Convert a draft into a project work item; the payload carries the draft's
 * fields client-side (the server reads the project off the draft). Resolves
 * with the created issue on HTTP 201.
 */
export async function serverConvertDraft(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  body: Record<string, unknown> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/draft-to-issue/${draftId}/`,
    sessionCookie,
    body
  );
  if (res.status !== 201)
    throw new Error(`[parity] draft convert failed with HTTP ${res.status}: ${(await res.text()).slice(0, 200)}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] converted issue carried no string id/name.");
  return { id: rec["id"], name: rec["name"] };
}

/** Convert a draft; resolves with the raw status for refusal-path assertions. */
export async function serverConvertDraftStatus(
  workspaceSlug: string,
  draftId: string,
  sessionCookie: string,
  body: Record<string, unknown> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/draft-to-issue/${draftId}/`,
    sessionCookie,
    body
  );
  return { status: res.status, bodyText: (await res.text()).slice(0, 300) };
}

/** One cursor page of the workspace drafts list, with the server's paging info. */
export interface ParityDraftsPage {
  status: number;
  drafts: ParityServerDraft[];
  total: number | null;
  nextCursor: string | null;
}

/**
 * Read one page of the drafts list with explicit cursor parameters (the UI
 * sends `per_page` plus `cursor`). Parsing stays tolerant: the scenarios
 * assert row contents, paging continuation only where the server reports it.
 */
export async function serverDraftsPage(
  workspaceSlug: string,
  sessionCookie: string,
  query: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityDraftsPage> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/draft-issues/${query}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] drafts page read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload["results"] as unknown[]) ?? []);
  const drafts = rows.map((row) => draftOf(row as Record<string, unknown>));
  const info = (payload["pagination_info"] ?? payload) as Record<string, unknown>;
  const totalRaw = info["total_results"] ?? info["count"] ?? payload["count"];
  const cursorRaw = info["next_cursor"] ?? payload["next_cursor"];
  return {
    status: res.status,
    drafts,
    total: typeof totalRaw === "number" ? totalRaw : null,
    nextCursor: typeof cursorRaw === "string" ? cursorRaw : null,
  };
}

/** A file asset row as the file-assets endpoints report it. */
export interface ParityFileAsset {
  id: string;
  assetKey: string;
  draftIssueId: string | null;
  issueId: string | null;
  entityType: string | null;
}

function assetOf(row: Record<string, unknown>, workspaceId: string): ParityFileAsset {
  if (typeof row["id"] !== "string") throw new Error("[parity] asset row carried no string id.");
  const asset = typeof row["asset"] === "string" ? row["asset"] : "";
  // The serializer resolves the storage key against the request path, so a
  // read-back value repeats the workspace/key segments (request path plus
  // the stored `{workspaceId}/{leaf}`); the true key is always the
  // trailing pair, and the read-back route takes only the leaf.
  const segments =
    asset
      .split("?")[0]
      ?.split("/")
      .filter((part) => part.length > 0) ?? [];
  const tail = segments.slice(-2);
  const key = tail.length === 2 && tail[0] === workspaceId ? (tail[1] as string) : (segments.at(-1) ?? "");
  const strOrNull = (value: unknown): string | null => (typeof value === "string" ? value : null);
  return {
    id: row["id"],
    assetKey: key,
    draftIssueId: strOrNull(row["draft_issue"] ?? row["draft_issue_id"]),
    issueId: strOrNull(row["issue"] ?? row["issue_id"]),
    entityType: strOrNull(row["entity_type"]),
  };
}

/**
 * Attach a file to a workspace draft through the same signed-URL flow the
 * description editor uses: create the asset row (linked to the draft),
 * PUT the bytes to object storage, flip the upload status. Resolves with
 * the stored row, including the storage key the read-back needs. Only
 * image types are accepted, like the editor.
 */
export async function serverUploadDraftAsset(
  workspaceSlug: string,
  workspaceId: string,
  projectId: string,
  draftId: string,
  sessionCookie: string,
  file: { name: string; mime: string; bytes: Uint8Array },
  apiBase: string = apiBaseFromEnv()
): Promise<ParityFileAsset> {
  const created = await mutateJSON(
    "POST",
    `${apiBase}/api/assets/v2/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    {
      name: file.name,
      type: file.mime,
      size: file.bytes.length,
      entity_type: "DRAFT_ISSUE_DESCRIPTION",
      entity_identifier: draftId,
    }
  );
  if (created.status !== 200 && created.status !== 201)
    throw new Error(
      `[parity] draft asset row create failed with HTTP ${created.status}: ${(await created.text()).slice(0, 200)}.`
    );
  const signed = (await created.json()) as {
    asset_id: string;
    upload_data: { url: string; fields: Record<string, string> };
  };
  if (typeof signed.asset_id !== "string" || typeof signed.upload_data?.url !== "string")
    throw new Error("[parity] draft asset row create returned no signed URL.");
  const form = new FormData();
  for (const [key, value] of Object.entries(signed.upload_data.fields ?? {})) form.append(key, value);
  form.append("file", new Blob([file.bytes], { type: file.mime }), file.name);
  // The server signs the upload against the request host, so a direct API
  // call yields an API-origin URL that Django cannot serve (404); only the
  // parity proxy routes the bucket path to minio. Re-host the signed URL
  // there — the signature covers the fields, not the host.
  const uploadUrl = (() => {
    try {
      const proxy = new URL(parityOracleUrl(process.env));
      const target = new URL(signed.upload_data.url);
      target.protocol = proxy.protocol;
      target.host = proxy.host;
      return target.toString();
    } catch {
      return signed.upload_data.url;
    }
  })();
  // Object storage is stubbed on the shared runners (the minio image
  // resolves to `sleep infinity`, so the proxy answers 502); the asset
  // records, flip and convert-carry the scenarios assert are pure
  // database rows and verify identically without bytes. Upload bytes when
  // the store answers, but keep application errors strict: only a dead
  // store (refused/502/503/504) falls back to record-only.
  let stored: Response | null = null;
  try {
    stored = await fetch(uploadUrl, { method: "POST", body: form });
  } catch {
    stored = null;
  }
  if (stored === null || stored.status === 502 || stored.status === 503 || stored.status === 504) {
    console.warn(
      `[parity] object store unreachable (status ${stored === null ? "refused" : stored.status}); continuing record-only.`
    );
  } else if (!stored.ok) {
    throw new Error(`[parity] draft asset byte upload failed with HTTP ${stored.status}.`);
  }
  const flipped = await mutateJSON(
    "PATCH",
    `${apiBase}/api/assets/v2/workspaces/${workspaceSlug}/projects/${projectId}/${signed.asset_id}/`,
    sessionCookie,
    {}
  );
  if (!flipped.ok) throw new Error(`[parity] draft asset status flip failed with HTTP ${flipped.status}.`);
  const key = signed.upload_data.fields?.["key"] ?? "";
  const at = key.indexOf(`${workspaceId}/`);
  const assetKey = at < 0 ? key : key.slice(at + workspaceId.length + 1);
  return serverFileAsset(workspaceId, assetKey, sessionCookie, apiBase);
}

/** Read a file asset back by its storage key. */
export async function serverFileAsset(
  workspaceId: string,
  assetKey: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityFileAsset> {
  const res = await fetchShared(`${apiBase}/api/workspaces/file-assets/${workspaceId}/${assetKey}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] file asset read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { data?: unknown };
  const rows = Array.isArray(payload.data) ? payload.data : [];
  if (rows.length === 0) throw new Error("[parity] file asset read returned no rows.");
  return assetOf(rows[0] as Record<string, unknown>, workspaceId);
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

/**
 * Mark a session as desktop (`pidash_client=desktop`) in the session store.
 *
 * NEWFRONT-595: the desktop-mode gate plants web-session cookies for legs that
 * must run behind a desktop-marked session (the exchange hand-off that would
 * set the marker lives on the cloud server and is absent here — DESK-014).
 * Simulates a completed desktop hand-off; mirrors the marker reader
 * `serverDeskSigninSessionHasDesktopMarker`.
 */
export async function serverDesktopRuntimeMarkSessionDesktop(sessionCookie: string): Promise<void> {
  const key = /(?:^|;\s*)session-id=([^;]+)/.exec(sessionCookie)?.[1] ?? "";
  if (key === "") throw new Error("[parity] session cookie carries no session-id.");
  await desktopRuntimeShell(
    `from pi_dash.db.models.session import Session\n` +
      `row = Session.objects.filter(session_key=${JSON.stringify(key)}).first()\n` +
      `assert row is not None, "session row missing"\n` +
      `data = row.get_decoded()\n` +
      `data["pidash_client"] = "desktop"\n` +
      `row.session_data = Session.objects.encode(data)\n` +
      `row.save(update_fields=["session_data"])\n` +
      `print("PARITY_DESK_MARK_DESKTOP:ok")\n`
  );
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

// --- Live cycles list markers (NEWFRONT-249, CYC-002). Appended; existing
// --- helpers above are untouched per the shared harness contract.
/** Mark a cycle as the caller's favorite (row-marker fixture setup). */
export async function serverAddCycleFavorite(
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
  if (!res.ok) throw new Error(`[parity] cycle favorite add failed with HTTP ${res.status}.`);
}

/**
 * Archived-issues list read without throwing (NEWFRONT-223, ARCH-008
 * guest branch): status plus the rows when the read succeeds. Guests
 * are refused at the endpoint, so the status is the assertion.
 */
export async function serverArchivedIssuesStatus(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; rows: { id: string; name: string }[] }> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/archived-issues/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) return { status: res.status, rows: [] };
  const payload: unknown = await res.json();
  const raw: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const rows = raw.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string") {
      throw new Error("[parity] archived row carried no string id and name.");
    }
    return { id: record.id, name: record.name };
  });
  return { status: res.status, rows };
}

// --- Runners overview / pods helpers (NEWFRONT-178, RUN-001/005, RUN-010/015).
// --- Appended; existing helpers above are untouched per the shared harness
// --- contract. Session-authenticated like the old runners screens; the v1
// --- create helper speaks the CLI-enrolment endpoint with a minted API key.

/** Non-privileged seeded identity for the access-control scenarios. */
export interface ParityGuestSeed {
  email: string;
  password: string;
}

/** The seeded guest, or a thrown error naming the missing seed step. */
export function requireGuestSeed(seed: ParitySeedFacts): ParityGuestSeed {
  if (seed.guestEmail === undefined || seed.guestPassword === undefined) {
    throw new Error("[parity] seed facts carry no guestEmail; re-run the stack seed step (see stack/README.md).");
  }
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Workspace UUID the runner/pod endpoints scope on (from the project read). */
export async function serverWorkspaceId(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const { record } = await projectFacts(workspaceSlug, projectId, sessionCookie, apiBase);
  const workspace = record["workspace"];
  if (typeof workspace !== "string" || workspace === "") {
    throw new Error("[parity] project row carried no string workspace id.");
  }
  return workspace;
}

/** One runner row as the list/detail endpoints report it. */
export interface ParityRunnerRow {
  id: string;
  name: string;
  status: string;
  pod: string;
  podName: string;
  enrolled: boolean;
  os: string;
  arch: string;
  version: string;
  heartbeat: string | null;
}

function runnerRowOf(row: Record<string, unknown>): ParityRunnerRow {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string" || typeof row["status"] !== "string") {
    throw new Error("[parity] runner row carried no string id/name/status.");
  }
  const pod = row["pod"];
  const detail = row["pod_detail"] as { name?: unknown } | null;
  return {
    id: row["id"],
    name: row["name"],
    status: row["status"],
    pod: typeof pod === "string" ? pod : "",
    podName: typeof detail?.name === "string" ? detail.name : "",
    enrolled: row["enrolled_at"] !== null,
    os: typeof row["os"] === "string" ? row["os"] : "",
    arch: typeof row["arch"] === "string" ? row["arch"] : "",
    version: typeof row["runner_version"] === "string" ? row["runner_version"] : "",
    heartbeat: typeof row["last_heartbeat_at"] === "string" ? row["last_heartbeat_at"] : null,
  };
}

/** Runners visible to the session, optionally narrowed to one project. */
export async function serverRunners(
  workspaceId: string,
  sessionCookie: string,
  projectId?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityRunnerRow[]> {
  const query = projectId === undefined ? `workspace=${workspaceId}` : `workspace=${workspaceId}&project=${projectId}`;
  const res = await authedApi(`/runners/?${query}`, sessionCookie, undefined, apiBase);
  requireOk(res, "runner list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map(runnerRowOf);
}

/** One runner, or null when the server reports it gone. */
export async function serverRunnerOrNull(
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityRunnerRow | null> {
  const res = await authedApi(`/runners/${runnerId}/`, sessionCookie, undefined, apiBase);
  if (res.status === 404) return null;
  requireOk(res, "runner read");
  return runnerRowOf((await res.json()) as Record<string, unknown>);
}

/** A minted user API key (raw token is only visible at creation). */
export interface ParityApiToken {
  id: string;
  token: string;
}

/** Mint a user API key through the session-authenticated endpoint. */
export async function serverMintApiToken(
  sessionCookie: string,
  label: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityApiToken> {
  const res = await authedApi(
    `/users/api-tokens/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ label }) },
    apiBase
  );
  requireOk(res, "api-token mint");
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["token"] !== "string") {
    throw new Error("[parity] api-token create response carried no id/token.");
  }
  return { id: rec["id"], token: rec["token"] };
}

/** Delete a minted API key (best effort: 404 means it is already gone). */
export async function serverDeleteApiToken(
  tokenId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await authedApi(`/users/api-tokens/${tokenId}/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`[parity] api-token delete failed with HTTP ${res.status}.`);
  }
}

/** Runner minted through the CLI-enrolment (v1) endpoint. */
export interface ParityCreatedRunner {
  runnerId: string;
  runnerName: string;
  podSlug: string;
}

/**
 * Create a runner the way `pidash runner add` does: X-Api-Key against the
 * v1 endpoint. Lands enrolled in the named pod (full server-side name) or
 * the project's default pod when omitted.
 */
export async function serverCreateRunnerV1(
  apiToken: string,
  input: { project: string; workspaceSlug?: string; pod?: string; name?: string; hostLabel?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCreatedRunner> {
  const body: Record<string, string> = { project: input.project };
  if (input.workspaceSlug !== undefined) body["workspace_slug"] = input.workspaceSlug;
  if (input.pod !== undefined) body["pod"] = input.pod;
  if (input.name !== undefined) body["name"] = input.name;
  if (input.hostLabel !== undefined) body["host_label"] = input.hostLabel;
  const res = await fetch(`${apiBase}/api/v1/runner/runners/`, {
    method: "POST",
    headers: { "X-Api-Key": apiToken, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`[parity] v1 runner create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["runner_id"] !== "string" || typeof rec["runner_name"] !== "string") {
    throw new Error("[parity] v1 runner create response carried no runner id/name.");
  }
  return {
    runnerId: rec["runner_id"],
    runnerName: rec["runner_name"],
    podSlug: typeof rec["pod_slug"] === "string" ? rec["pod_slug"] : "",
  };
}

/** Hard-delete a runner; resolves with the HTTP status (204 on success). */
export async function serverDeleteRunner(
  runnerId: string,
  sessionCookie: string,
  purgeLocal = false,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await authedApi(
    `/runners/${runnerId}/?purge_local=${purgeLocal ? "true" : "false"}`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
  return res.status;
}

/** Revoke a runner; resolves with its post-revoke status string. */
export async function serverRevokeRunner(
  runnerId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await authedApi(
    `/runners/${runnerId}/revoke/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
  requireOk(res, "runner revoke");
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["status"] !== "string") throw new Error("[parity] runner revoke response carried no status.");
  return rec["status"];
}

/** Rename and/or move a runner; resolves with the updated row. */
export async function serverPatchRunner(
  runnerId: string,
  sessionCookie: string,
  patch: { name?: string; pod?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<ParityRunnerRow> {
  const res = await authedApi(
    `/runners/${runnerId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  requireOk(res, "runner patch");
  return runnerRowOf((await res.json()) as Record<string, unknown>);
}

/** One pod as the pod endpoints report it. */
export interface ParityPodRow {
  id: string;
  name: string;
  description: string;
  isDefault: boolean;
  project: string;
  projectIdentifier: string;
  runnerCount: number;
}

function podRowOf(row: Record<string, unknown>): ParityPodRow {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string") {
    throw new Error("[parity] pod row carried no string id/name.");
  }
  return {
    id: row["id"],
    name: row["name"],
    description: typeof row["description"] === "string" ? row["description"] : "",
    isDefault: row["is_default"] === true,
    project: typeof row["project"] === "string" ? row["project"] : "",
    projectIdentifier: typeof row["project_identifier"] === "string" ? row["project_identifier"] : "",
    runnerCount: typeof row["runner_count"] === "number" ? row["runner_count"] : 0,
  };
}

/** Every pod of one project, in API order. */
export async function serverPods(
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPodRow[]> {
  const res = await authedApi(`/runners/pods/?project=${projectId}`, sessionCookie, undefined, apiBase);
  requireOk(res, "pod list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map(podRowOf);
}

/** Every pod of one workspace, in API order (what the workspace overview renders). */
export async function serverWorkspacePods(
  workspaceId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPodRow[]> {
  const res = await authedApi(`/runners/pods/?workspace=${workspaceId}`, sessionCookie, undefined, apiBase);
  requireOk(res, "pod list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map(podRowOf);
}

/** Rename / re-describe / promote a pod; resolves with the updated row. */
export async function serverPatchPod(
  podId: string,
  sessionCookie: string,
  patch: { name?: string; description?: string; is_default?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPodRow> {
  const res = await authedApi(
    `/runners/pods/${podId}/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) },
    apiBase
  );
  requireOk(res, "pod patch");
  return podRowOf((await res.json()) as Record<string, unknown>);
}

/** Pod-delete outcome including the machine-readable guard code. */
export interface ParityPodDeleteResult {
  status: number;
  code: string | null;
  error: string;
}

/** Delete a pod without throwing, so scenarios can assert the guard codes. */
export async function serverDeletePodResult(
  podId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPodDeleteResult> {
  const res = await authedApi(`/runners/pods/${podId}/`, sessionCookie, { method: "DELETE" }, apiBase);
  if (res.status === 204) return { status: 204, code: null, error: "" };
  let code: string | null = null;
  let error = "";
  try {
    const rec = (await res.json()) as Record<string, unknown>;
    code = typeof rec["code"] === "string" ? rec["code"] : null;
    error = typeof rec["error"] === "string" ? rec["error"] : "";
  } catch {
    code = null;
  }
  return { status: res.status, code, error };
}

/** A created agent run: id, status, and the pod the delegation landed in. */
export interface ParityCreatedRun {
  id: string;
  status: string;
  pod: string;
}

/** Create a run; the server resolves the pod (explicit > pin > default). */
export async function serverCreateRun(
  sessionCookie: string,
  input: { workspaceId: string; prompt: string; workItemId?: string; podId?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCreatedRun> {
  const body: Record<string, string> = { workspace: input.workspaceId, prompt: input.prompt };
  if (input.workItemId !== undefined) body["work_item"] = input.workItemId;
  if (input.podId !== undefined) body["pod"] = input.podId;
  const res = await authedApi(
    `/runners/runs/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`[parity] run create failed with HTTP ${res.status}: ${text.slice(0, 200)}`);
  }
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["status"] !== "string" || typeof rec["pod"] !== "string") {
    throw new Error("[parity] run create response carried no id/status/pod.");
  }
  return { id: rec["id"], status: rec["status"], pod: rec["pod"] };
}

/** Cancel a run; resolves with the post-cancel status string. */
export async function serverCancelRun(
  runId: string,
  sessionCookie: string,
  reason: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await authedApi(
    `/runners/runs/${runId}/cancel/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ reason }) },
    apiBase
  );
  requireOk(res, "run cancel");
  const rec = (await res.json()) as Record<string, unknown>;
  return typeof rec["status"] === "string" ? rec["status"] : "";
}

/**
 * The pod pin on one issue (null when unpinned). New issues auto-pin to
 * the project default on save, so delegation scenarios clear first.
 */
export async function serverIssuePodPin(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/${issueId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "issue read");
  const rec = (await res.json()) as Record<string, unknown>;
  const pin = rec["assigned_pod_id"];
  return typeof pin === "string" ? pin : null;
}

/** Clear one issue's pod pin so its runs follow the project default. */
export async function serverUnpinIssuePod(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await patchIssue(workspaceSlug, projectId, issueId, sessionCookie, { assigned_pod_id: null }, apiBase);
}

// --- Intake detail (NEWFRONT-258, INT-019/024/026-029). Observed against
// --- the running old app: issue fields PATCH nested under `issue` while
// --- the intake status PATCHes top-level; the retrieve nests the full
// --- work-item detail (description, priority, assignees, labels, dates);
// --- description versions list metadata only and retrieve one version's
// --- stored markup. Guests may only write name/description server-side.

/** Field snapshot of an intake issue's nested work item. */
export interface ParityServerIntakeDetail {
  name: string;
  descriptionHtml: string;
  priority: string | null;
  assigneeIds: string[];
  labelIds: string[];
  targetDate: string | null;
  status: number;
}

/** One description-version list row (metadata; markup needs retrieve). */
export interface ParityServerIntakeDetailVersion {
  id: string;
  ownedBy: string;
  createdAt: string;
}

/**
 * Raw PATCH against an intake issue. Never throws on 4xx: refusal
 * assertions (INT-026) read the status instead of catching.
 */
export async function serverIntakeDetailPatch(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  data: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    data
  );
  return { status: res.status, bodyText: await res.text() };
}

/** Set the intake status (resolved fixtures); throws unless the write lands. */
export async function serverIntakeDetailSetStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  status: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    { status }
  );
  if (!res.ok) throw new Error(`[parity] intake status patch failed with HTTP ${res.status}.`);
}

/** Field snapshot of an intake issue, read by the nested issue id. */
export async function serverIntakeDetailRead(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerIntakeDetail> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] intake-detail read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (issue === undefined) throw new Error("[parity] intake-detail read carried no nested issue.");
  const strArray = (key: string): string[] => {
    const value = issue[key];
    return Array.isArray(value) ? value.map(String) : [];
  };
  return {
    name: typeof issue["name"] === "string" ? (issue["name"] as string) : "",
    descriptionHtml: typeof issue["description_html"] === "string" ? (issue["description_html"] as string) : "",
    priority: typeof issue["priority"] === "string" ? (issue["priority"] as string) : null,
    assigneeIds: strArray("assignee_ids"),
    labelIds: strArray("label_ids"),
    targetDate: typeof issue["target_date"] === "string" ? (issue["target_date"] as string) : null,
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
  };
}

/** Description versions of an intake work item, newest first. */
export async function serverIntakeDetailVersions(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerIntakeDetailVersion[]> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-work-items/${issueId}/description-versions/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] intake versions read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const record = row as Record<string, unknown>;
    return {
      id: typeof record["id"] === "string" ? (record["id"] as string) : "",
      ownedBy: typeof record["owned_by"] === "string" ? (record["owned_by"] as string) : "",
      createdAt: typeof record["created_at"] === "string" ? (record["created_at"] as string) : "",
    };
  });
}

/**
 * Create a listed attachment on an intake work item: the metadata POST
 * plus the uploaded-status PATCH the client sends after its byte PUT.
 * The scratch stack's object storage is stubbed on this machine, so no
 * bytes are stored — the row still lists and renders like an upload.
 */
export async function serverIntakeDetailCreateAttachment(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  name: string,
  mime: string,
  size: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const post = await mutateJSON(
    "POST",
    `${apiBase}/api/assets/v2/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/attachments/`,
    sessionCookie,
    { name, type: mime, size }
  );
  if (!post.ok) throw new Error(`[parity] attachment metadata post failed with HTTP ${post.status}.`);
  const rec = (await post.json()) as Record<string, unknown>;
  if (typeof rec["asset_id"] !== "string") throw new Error("[parity] attachment post carried no asset id.");
  const assetId = rec["asset_id"] as string;
  const mark = await mutateJSON(
    "PATCH",
    `${apiBase}/api/assets/v2/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/attachments/${assetId}/`,
    sessionCookie
  );
  if (!mark.ok && mark.status !== 204)
    throw new Error(`[parity] attachment status patch failed with HTTP ${mark.status}.`);
  return assetId;
}

/** Stored markup of one description version. */
export async function serverIntakeDetailVersion(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  versionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ descriptionHtml: string }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-work-items/${issueId}/description-versions/${versionId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] intake version read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return { descriptionHtml: typeof rec["description_html"] === "string" ? (rec["description_html"] as string) : "" };
}
// ---------------------------------------------------------------------------
// Intake-create oracle helpers (NEWFRONT-257, rows INT-013–017/033).
//
// Appended; existing helpers above are untouched per the shared contract.
// The create dialog posts the work item nested under `issue` and forces the
// triage state server-side, so validation probes post raw payloads and read
// the status back instead of throwing, and the detail read carries the
// intake source plus the nested issue fields the create rows assert on.
// ---------------------------------------------------------------------------

/** Raw intake-create answer: HTTP status plus the parsed JSON body. */
export interface ServerIntakeCreateRawResult {
  status: number;
  body: unknown;
}

/**
 * POST a raw issue payload to the intake-create endpoint without throwing
 * on 4xx, so validation scenarios can assert the refusal (empty name and
 * out-of-range priority both answer 400 with nothing created).
 */
export async function serverIntakeCreateRaw(
  workspaceSlug: string,
  projectId: string,
  issue: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeCreateRawResult> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/`,
    sessionCookie,
    { source: "IN_APP", issue }
  );
  let body: unknown = null;
  try {
    body = (await res.json()) as unknown;
  } catch {
    body = null;
  }
  return { status: res.status, body };
}

/** One created intake request as the server reports it (source plus nested issue fields). */
export interface ServerIntakeCreateDetail {
  inboxId: string;
  issueId: string;
  status: number;
  source: string;
  name: string;
  descriptionHtml: string;
  priority: string | null;
  stateId: string;
  targetDate: string | null;
}

/** One intake request by its nested issue id: intake row plus nested issue fields. */
export async function serverIntakeCreateDetail(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeCreateDetail> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] inbox-issue read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (typeof rec["id"] !== "string" || issue === undefined || typeof issue["id"] !== "string")
    throw new Error("[parity] inbox-issue detail carried no inbox/issue ids.");
  const strOrNull = (row: Record<string, unknown>, key: string): string | null => {
    const value = row[key];
    return typeof value === "string" ? value : null;
  };
  return {
    inboxId: rec["id"] as string,
    issueId: issue["id"] as string,
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    source: typeof rec["source"] === "string" ? (rec["source"] as string) : "",
    name: typeof issue["name"] === "string" ? (issue["name"] as string) : "",
    descriptionHtml: typeof issue["description_html"] === "string" ? (issue["description_html"] as string) : "",
    priority: strOrNull(issue, "priority"),
    stateId: typeof issue["state_id"] === "string" ? (issue["state_id"] as string) : "",
    targetDate: strOrNull(issue, "target_date"),
  };
}

/** One file asset linked to a work item (entity kind plus upload flag). */
export interface ServerIntakeCreateIssueAsset {
  id: string;
  entityType: string;
  uploaded: boolean;
}

/**
 * File assets linked to one work item. The attachments REST list only
 * shows ISSUE_ATTACHMENT rows, while composed images are
 * ISSUE_DESCRIPTION, so the bulk-link proof reads through the shell.
 */
export async function serverIntakeCreateIssueAssets(issueId: string): Promise<ServerIntakeCreateIssueAsset[]> {
  const out = await apiShell(
    `from pi_dash.db.models.asset import FileAsset\n` +
      `for a in FileAsset.objects.filter(issue_id=${JSON.stringify(issueId)}):\n` +
      `    print("PARITY_ASSET:" + str(a.id) + "|" + str(a.entity_type) + "|" + str(a.is_uploaded))\n`
  );
  return out
    .split("\n")
    .map((line) => /^PARITY_ASSET:(.+)\|(.+)\|(.+)$/.exec(line.trim()))
    .filter((match) => match !== null)
    .map((match) => ({
      id: (match as RegExpExecArray)[1] as string,
      entityType: (match as RegExpExecArray)[2] as string,
      uploaded: (match as RegExpExecArray)[3] === "True",
    }));
}

/** Triage-state probe: 200 carries the state, 404 carries no state (never throws on 404). */
export interface ServerIntakeCreateTriageProbe {
  status: number;
  id: string | null;
  name: string | null;
}

/**
 * Delete the project's triage state through the Django shell. The states
 * REST API excludes triage rows (its queryset filters them out), so no
 * REST delete exists; the shell is the documented removal path for the
 * INT-033 auto-creation proof. Resolves with the deleted row count.
 */
export async function serverIntakeCreateDeleteTriage(projectId: string): Promise<number> {
  const out = await apiShell(
    `from pi_dash.db.models import State\n` +
      `result = State.triage_objects.filter(project_id=${JSON.stringify(projectId)}).delete()\n` +
      `deleted = result[0] if isinstance(result, tuple) else result\n` +
      `print("PARITY_DELETED:" + str(deleted))\n`
  );
  const count = /^PARITY_DELETED:(\d+)$/m.exec(out)?.[1] ?? "";
  if (count === "") throw new Error("[parity] triage delete produced no count.");
  return Number(count);
}

/** Project triage state read that tolerates the missing-state 404 (INT-033 auto-creation proof). */
export async function serverIntakeCreateTriageProbe(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeCreateTriageProbe> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/intake-state/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return { status: 404, id: null, name: null };
  if (!res.ok) throw new Error(`[parity] intake-state read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return {
    status: res.status,
    id: typeof rec["id"] === "string" ? rec["id"] : null,
    name: typeof rec["name"] === "string" ? rec["name"] : null,
  };
}

/**
 * Overlap verdict for a candidate cycle date range (NEWFRONT-250, CYC-016):
 * the same POST date-check the create dialog runs before saving. Resolves
 * true when the range is free, false when it overlaps an existing cycle.
 */
export async function serverCycleDateCheck(
  workspaceSlug: string,
  projectId: string,
  payload: { start_date: string; end_date: string; cycle_id?: string },
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/date-check/`,
    sessionCookie,
    payload
  );
  if (!res.ok) throw new Error(`[parity] cycle date-check failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["status"] !== "boolean") throw new Error("[parity] date-check carried no boolean status.");
  return rec["status"] as boolean;
}

/** Whether the project's cycles feature flag reads enabled (NEWFRONT-250, CYC-010). */
export async function serverProjectCycleView(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["cycle_view"] !== "boolean") throw new Error("[parity] project carried no boolean cycle_view.");
  return rec["cycle_view"] as boolean;
}

// --- NEWFRONT-253 (cycles detail/sidebar, CYC-032-039): per-cycle view
// --- (serverCycleDateCheckEdit: NEWFRONT-250 owns the payload-style
// --- serverCycleDateCheck name; this positional-args twin keeps its own.)
// --- preferences, date overlap checks, progress analytics, and the full
// --- cycle record. Appended; existing helpers above are untouched.

/** Per-cycle view preferences as the server stores them per user. */
export interface ParityCycleUserProperties {
  /** Stored layout key (list/kanban/calendar/spreadsheet/gantt), if any. */
  layout: string | null;
  /** Stored rich filter expression (work-item filters), if any. */
  richFilters: unknown;
  /** Legacy flat filters map. */
  filters: Record<string, unknown>;
  /** Raw display-filters payload (layout lives inside), if any. */
  displayFilters: unknown;
}

/** Read one cycle's per-user view preferences (layout + filters). */
/** Read the caller's cycle user-properties (rich_filters prove hero breakdown clicks). */
export async function serverCycleUserProperties(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCycleUserProperties> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/user-properties/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle user-properties read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const display = rec["display_filters"] as Record<string, unknown> | undefined;
  return {
    layout: typeof display?.["layout"] === "string" ? (display["layout"] as string) : null,
    richFilters: rec["rich_filters"] ?? null,
    filters: (rec["filters"] as Record<string, unknown> | undefined) ?? {},
    displayFilters: rec["display_filters"] ?? null,
  };
}

/** Ask the server whether a date range overlaps another cycle (overlap guard). */
export async function serverCycleDateCheckEdit(
  workspaceSlug: string,
  projectId: string,
  startDate: string,
  endDate: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/date-check/`,
    sessionCookie,
    { start_date: startDate, end_date: endDate, cycle_id: cycleId }
  );
  if (!res.ok) throw new Error(`[parity] cycle date-check failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return rec["status"] === true;
}

/** One analytics distribution row (assignee or label share). */
export interface ParityCycleDistributionRow {
  title: string;
  completed: number;
  total: number;
}

/** Cycle progress analytics as the server computes them. */
export interface ParityCycleAnalytics {
  assignees: ParityCycleDistributionRow[];
  labels: ParityCycleDistributionRow[];
  /** Dated entries in the completion chart (0 when the chart has no data). */
  completionDays: number;
}

/** Read one cycle's progress analytics (the sidebar's numbers). */
export async function serverCycleAnalytics(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  measure: "issues" | "points",
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCycleAnalytics> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/analytics/?type=${measure}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle analytics read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const rows = (
    key: string,
    titleKey: string,
    completedKey: string,
    totalKey: string
  ): ParityCycleDistributionRow[] => {
    const raw = rec[key];
    if (!Array.isArray(raw)) return [];
    return raw.flatMap((row) => {
      const entry = row as Record<string, unknown>;
      return typeof entry[titleKey] === "string" &&
        typeof entry[completedKey] === "number" &&
        typeof entry[totalKey] === "number"
        ? [{ title: entry[titleKey] as string, completed: entry[completedKey], total: entry[totalKey] }]
        : [];
    });
  };
  const chart = rec["completion_chart"] as Record<string, unknown> | undefined;
  return {
    assignees: rows("assignees", "display_name", "completed_issues", "total_issues"),
    labels: rows("labels", "label_name", "completed_issues", "total_issues"),
    completionDays: chart === undefined ? 0 : Object.keys(chart).length,
  };
}

/** Full cycle record fields the detail sidebar renders. */
export interface ParityCycleDetailFull {
  name: string;
  description: string;
  ownedById: string;
  startDate: string | null;
  endDate: string | null;
}

/** One module's server-side record as the row-actions scenarios assert it (NEWFRONT-308). */
export interface ModulesDetail {
  /** Module name. */
  name: string;
  /** Lifecycle status value (backlog, planned, in-progress, paused, completed, cancelled). */
  status: string;
  /** Saved start date (YYYY-MM-DD) or null when empty. */
  startDate: string | null;
  /** Saved target date (YYYY-MM-DD) or null when empty. */
  targetDate: string | null;
  /** Lead user id or null when no lead is set. */
  leadId: string | null;
  /** Whether the requesting user favorited the module. */
  isFavorite: boolean;
  /** Manual-placement order value. */
  sortOrder: number;
  /** Archive timestamp or null when active. */
  archivedAt: string | null;
}

/**
 * Read one module's full record (status, dates, lead, favorite, order).
 * The modules list payload omits some of these, so row-actions scenarios
 * read the detail endpoint after every inline edit.
 */
export async function modulesFetchModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ModulesDetail> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] module read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["name"] !== "string" || typeof rec["status"] !== "string")
    throw new Error("[parity] module record carried no string name/status.");
  return {
    name: rec["name"],
    status: rec["status"],
    startDate: typeof rec["start_date"] === "string" ? rec["start_date"] : null,
    targetDate: typeof rec["target_date"] === "string" ? rec["target_date"] : null,
    leadId: typeof rec["lead_id"] === "string" ? rec["lead_id"] : null,
    isFavorite: rec["is_favorite"] === true,
    sortOrder: typeof rec["sort_order"] === "number" ? rec["sort_order"] : 0,
    archivedAt: typeof rec["archived_at"] === "string" ? rec["archived_at"] : null,
  };
}

/** Read one cycle's full record (description, lead, dates). */
export async function serverCycleDetailFull(
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityCycleDetailFull> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] cycle read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["name"] !== "string" || typeof rec["owned_by_id"] !== "string")
    throw new Error("[parity] cycle record carried no string name/owned_by_id.");
  return {
    name: rec["name"],
    description: typeof rec["description"] === "string" ? rec["description"] : "",
    ownedById: rec["owned_by_id"],
    startDate: typeof rec["start_date"] === "string" ? rec["start_date"] : null,
    endDate: typeof rec["end_date"] === "string" ? rec["end_date"] : null,
  };
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

// --- Pages realtime collaboration + live server (NEWFRONT-319, PAGE-065-071).
// --- Appended; existing helpers above are untouched per the shared driver contract.
// --- Live HTTP goes through the slot proxy origin (/live/*), the same origin
// --- the editor's same-origin fallback dials, so no new env var is needed.

/** Oracle base URL (the slot proxy): live HTTP rides /live/* beside /api/*. */
export function pagesLiveOracleBase(): string {
  return parityOracleUrl(process.env).trim().replace(/\/+$/, "");
}

/** A project page record as the detail endpoint reports it. */
export interface PagesLivePageRecord {
  id: string;
  name: string;
  description_html: string | null;
  is_locked: boolean;
  archived_at: string | null;
}

/** Read one project page; throws naming the failure. */
export async function pagesLiveReadPage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesLivePageRecord> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as {
    id?: unknown;
    name?: unknown;
    description_html?: unknown;
    is_locked?: unknown;
    archived_at?: unknown;
  };
  if (typeof row.id !== "string" || typeof row.name !== "string") {
    throw new Error("[parity] page read carried no string id/name.");
  }
  return {
    id: row.id,
    name: row.name,
    description_html: typeof row.description_html === "string" ? row.description_html : null,
    is_locked: row.is_locked === true,
    archived_at: typeof row.archived_at === "string" ? row.archived_at : null,
  };
}

/** Lock or unlock a page through the dedicated endpoint; throws naming the failure. */
export async function pagesLiveSetPageLock(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  locked: boolean,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    locked ? "POST" : "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/lock/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] page lock patch failed with HTTP ${res.status}.`);
}

/**
 * PATCH a page description without throwing, for oversized/locked refusal
 * assertions (the status IS the expectation). Resolves with status + body.
 */
export async function pagesLivePatchDescriptionLenient(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  body: { description_binary?: string; description_html?: string; description_json?: unknown },
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/description/`,
    sessionCookie,
    body
  );
  return { status: res.status, bodyText: (await res.text()).slice(0, 500) };
}

/** Live liveness probe answer. */
export interface PagesLiveHealth {
  status: string;
  timestamp: string;
  version: string;
}

/** GET the live liveness probe through the proxy; throws naming the failure. */
export async function pagesLiveHealth(oracleBase: string = pagesLiveOracleBase()): Promise<PagesLiveHealth> {
  const res = await fetch(`${oracleBase}/live/health/`);
  if (!res.ok) throw new Error(`[parity] live health probe failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { status?: unknown; timestamp?: unknown; version?: unknown };
  if (
    typeof payload.status !== "string" ||
    typeof payload.timestamp !== "string" ||
    typeof payload.version !== "string"
  ) {
    throw new Error("[parity] live health probe carried no status/timestamp/version strings.");
  }
  return { status: payload.status, timestamp: payload.timestamp, version: payload.version };
}

/**
 * POST the live document-conversion endpoint without throwing (validation
 * failures are expectations). Resolves with status + parsed-or-raw body.
 */
export async function pagesLiveConvertDocumentLenient(
  body: unknown,
  oracleBase: string = pagesLiveOracleBase()
): Promise<{ status: number; payload: unknown }> {
  const res = await fetch(`${oracleBase}/live/convert-document/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  let payload: unknown = text;
  try {
    payload = JSON.parse(text) as unknown;
  } catch {
    // Non-JSON error bodies stay raw; the status carries the assertion.
  }
  return { status: res.status, payload };
}

/**
 * POST the live printable-export endpoint without throwing (each failure
 * class maps to a status). Resolves with status, content type, and either
 * the JSON error body or the leading bytes of the file.
 */
export async function pagesLivePdfExportLenient(
  body: unknown,
  sessionCookie: string,
  oracleBase: string = pagesLiveOracleBase()
): Promise<{ status: number; contentType: string; head: string; byteLength: number }> {
  const res = await fetch(`${oracleBase}/live/pdf-export/`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify(body),
  });
  const buffer = Buffer.from(await res.arrayBuffer());
  return {
    status: res.status,
    contentType: res.headers.get("content-type") ?? "",
    head: buffer.subarray(0, 200).toString("latin1"),
    byteLength: buffer.length,
  };
}

/**
 * GET a live path without throwing, for admin-surface absence assertions.
 * Extra headers ride along (e.g. the shared secret header).
 */
export async function pagesLiveGetLenient(
  path: string,
  headers: Record<string, string> = {},
  oracleBase: string = pagesLiveOracleBase()
): Promise<{ status: number; bodyText: string }> {
  const res = await fetch(`${oracleBase}${path}`, { headers });
  return { status: res.status, bodyText: (await res.text()).slice(0, 500) };
}

// --- Intake triage actions (NEWFRONT-259, INT-020/021/022/023/025).
// --- Appended; existing helpers above are untouched per the shared
// --- harness contract. Detail endpoints take the nested issue id, like
// --- the merged inbox helpers.

/** One intake row as the server reports it: triage fields plus the nested work item. */
export interface ServerIntakeTriageDetail {
  status: number;
  snoozedTill: string | null;
  duplicateTo: string | null;
  name: string;
  stateId: string;
  issueId: string;
  sequenceId: number;
}

/** One intake row as the server reports it, read by the nested issue id. */
export async function serverIntakeTriageDetail(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeTriageDetail> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] triage-issue read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  return {
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    snoozedTill: typeof rec["snoozed_till"] === "string" ? (rec["snoozed_till"] as string) : null,
    duplicateTo: typeof rec["duplicate_to"] === "string" ? (rec["duplicate_to"] as string) : null,
    name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
    stateId: typeof issue?.["state_id"] === "string" ? (issue["state_id"] as string) : "",
    issueId: typeof issue?.["id"] === "string" ? (issue["id"] as string) : "",
    sequenceId: typeof issue?.["sequence_id"] === "number" ? (issue["sequence_id"] as number) : 0,
  };
}

/** HTTP status of one intake-row retrieve (presence proofs after delete). */
export async function serverIntakeTriageInboxStatus(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    { headers: { cookie: sessionCookie } }
  );
  return res.status;
}

/** Raw triage PATCH (status / snoozed_till / duplicate_to); callers assert the status. */
export async function serverIntakeTriagePatchRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  body: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: string }> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    body
  );
  return { status: res.status, body: await res.text() };
}

/** Triage PATCH as an admin write; resolves with the stored row. */
export async function serverIntakeTriagePatch(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  body: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeTriageDetail> {
  const raw = await serverIntakeTriagePatchRaw(workspaceSlug, projectId, issueId, body, sessionCookie, apiBase);
  if (raw.status < 200 || raw.status >= 300)
    throw new Error(`[parity] triage patch failed with HTTP ${raw.status}: ${raw.body}`);
  return serverIntakeTriageDetail(workspaceSlug, projectId, issueId, sessionCookie, apiBase);
}

/** Raw intake-row DELETE; callers assert the status (refusals are 403). */
export async function serverIntakeTriageDeleteRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie
  );
  return res.status;
}

/** Whether the underlying work item still exists (cascade proofs after delete). */
export async function serverIntakeTriageWorkItemExists(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  return res.status === 200;
}

/** Duplicate-picker candidates as the search endpoint reports them. */
export async function serverIntakeTriageSearchIssues(
  workspaceSlug: string,
  projectId: string,
  query: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string; sequenceId: number }[]> {
  const params = new URLSearchParams({ search: query, workspace_search: "false" });
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/search-issues/?${params.toString()}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] triage issue search failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    return {
      id: typeof rec["id"] === "string" ? (rec["id"] as string) : "",
      name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
      sequenceId: typeof rec["sequence_id"] === "number" ? (rec["sequence_id"] as number) : 0,
    };
  });
}

// --- Pages prefs/assistant/cloud-stubs/desktop/negatives (NEWFRONT-320,
// --- PAGE-072-073, PAGE-075-078, PAGE-081-083). Appended; existing
// --- helpers above are untouched per the shared contract.
// --- serverPageRecord + serverUpdatePageDescription come from the sibling
// --- NEWFRONT-317 canonical copies (deduped at rebase per this block's
// --- original note); only the endpoint probes below are this area's own.

/**
 * Probe the workspace-scoped editor-task endpoint the assistant menu
 * would call. Resolves with the raw HTTP status the stack answers —
 * the oracle pins reachability, never invents a model reply.
 */
export async function pagesProbeEditorTask(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/rephrase-grammar/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ task: "ASK_ANYTHING", text_input: "parity probe" }),
  });
  return res.status;
}

/**
 * Probe the workspace AI-assistant root the conversational task would
 * call. Resolves with the raw HTTP status the stack answers.
 */
export async function pagesProbeAssistantRoot(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/ai-assistant/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ prompt: "parity probe", task: "ASK_ANYTHING" }),
  });
  return res.status;
}

// --- Intake cross-cutting (NEWFRONT-260, INT-018/030/031/032/034). Appended;
// --- existing helpers above are untouched per the shared parity contract.
// --- The retrieve endpoint takes the nested issue id; issue fields patch
// --- nested under `issue` while intake fields (status, snooze, duplicate
// --- target) patch flat; guests read only their own rows unless the project
// --- lets guests view everything.

/** Intake fields a cross-cutting scenario may set on an inbox row. */
export interface ServerIntakeXInboxPatch {
  status?: number;
  snoozed_till?: string | null;
  duplicate_to?: string;
  /** Nested issue rename (what the list and detail render as the title). */
  name?: string;
}

/** Patch an inbox row by its nested issue id; resolves with the stored status and title. */
export async function serverIntakeXPatchInboxIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  patch: ServerIntakeXInboxPatch,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; name: string }> {
  const body: Record<string, unknown> = {};
  if (patch.status !== undefined) body["status"] = patch.status;
  if (patch.snoozed_till !== undefined) body["snoozed_till"] = patch.snoozed_till;
  if (patch.duplicate_to !== undefined) body["duplicate_to"] = patch.duplicate_to;
  if (patch.name !== undefined) body["issue"] = { name: patch.name };
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    body
  );
  if (!res.ok) throw new Error(`[parity] inbox-issue patch failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (typeof rec["status"] !== "number") throw new Error("[parity] patched inbox issue carried no numeric status.");
  return {
    status: rec["status"] as number,
    name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
  };
}

/** One inbox row with its intake status, source and author. */
export interface ServerIntakeXInboxRow {
  issueId: string;
  name: string;
  status: number;
  source: string;
  createdBy: string;
}

/**
 * Inbox rows on a project, optionally narrowed to a status query (the
 * comma-joined form the UI sends, e.g. "-2"; omitted to let the server apply
 * its own default). Resolves with the rows plus the collection total.
 */
export async function serverIntakeXInboxIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  statusQuery?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ rows: ServerIntakeXInboxRow[]; total: number }> {
  const query = statusQuery === undefined ? "" : `?status=${encodeURIComponent(statusQuery)}`;
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${query}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] inbox-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const total =
    !Array.isArray(payload) && typeof (payload as { total_results?: unknown }).total_results === "number"
      ? (payload as { total_results: number }).total_results
      : rows.length;
  return {
    rows: rows.map((row) => {
      const rec = row as Record<string, unknown>;
      const issue = rec["issue"] as Record<string, unknown> | undefined;
      return {
        issueId: typeof issue?.["id"] === "string" ? (issue["id"] as string) : "",
        name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
        status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
        source: typeof rec["source"] === "string" ? (rec["source"] as string) : "",
        createdBy: typeof rec["created_by"] === "string" ? (rec["created_by"] as string) : "",
      };
    }),
    total,
  };
}

/** An inbox-issue retrieve that surfaces the HTTP status (guest refusals answer 403). */
export interface ServerIntakeXInboxRead {
  httpStatus: number;
  name: string;
  status: number;
  source: string;
  createdBy: string;
}

/** Read one inbox row by its nested issue id without throwing on refusal. */
export async function serverIntakeXReadInboxIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeXInboxRead> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    { headers: { cookie: sessionCookie } }
  );
  const empty: ServerIntakeXInboxRead = { httpStatus: res.status, name: "", status: 0, source: "", createdBy: "" };
  if (!res.ok) return empty;
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  const createdBy =
    typeof rec["created_by"] === "string"
      ? (rec["created_by"] as string)
      : typeof issue?.["created_by"] === "string"
        ? (issue["created_by"] as string)
        : "";
  return {
    httpStatus: res.status,
    name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    source: typeof rec["source"] === "string" ? (rec["source"] as string) : "",
    createdBy,
  };
}

/** Project flags the cross-cutting scenarios toggle. */
export interface ServerIntakeXProjectPatch {
  guest_view_all_features?: boolean;
}

/** Patch scenario-owned project flags (the guest-view-all toggle). */
export async function serverIntakeXPatchProject(
  workspaceSlug: string,
  projectId: string,
  patch: ServerIntakeXProjectPatch,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    patch
  );
  if (!res.ok) throw new Error(`[parity] project flags patch failed with HTTP ${res.status}.`);
}

/** Project flags the cross-cutting scenarios assert. */
export interface ServerIntakeXProjectFlags {
  inboxView: boolean;
  guestViewAll: boolean;
}

/** Read the intake-view and guest-view-all flags off a project. */
export async function serverIntakeXProjectFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeXProjectFlags> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return {
    inboxView: rec["inbox_view"] === true,
    guestViewAll: rec["guest_view_all_features"] === true,
  };
}

// --- Desktop bare sign-in, deep links, onboarding seam (NEWFRONT-208,
// --- DESK-011–015, DESK-027). Appended; existing helpers above are untouched
// --- per the shared driver contract.

/** Edge origin serving the oracle (browser-facing paths go through it). */
function deskSigninOracleBase(): string {
  const raw = parityOracleUrl(process.env).trim().replace(/\/+$/, "");
  if (raw === "") throw new Error("[parity] PARITY_ORACLE_URL is empty.");
  return raw;
}

/**
 * Raw answer of the desktop code-exchange route. By default this probes the
 * browser-facing address the native shell navigates its webview to
 * (`/api/auth/desktop-exchange/` through the edge proxy); `viaEdge: false`
 * probes the API service directly (`/auth/desktop-exchange/`).
 */
export async function serverDeskSigninDesktopExchangeStatus(input: {
  method?: "GET" | "POST";
  query?: string;
  sessionCookie?: string;
  viaEdge?: boolean;
}): Promise<{ status: number; body: string }> {
  const query = input.query ?? "code=parity-code&state=parity-state";
  const url =
    input.viaEdge === false
      ? `${apiBaseFromEnv()}/auth/desktop-exchange/?${query}`
      : `${deskSigninOracleBase()}/api/auth/desktop-exchange/?${query}`;
  const headers: Record<string, string> = {};
  if (input.sessionCookie !== undefined && input.sessionCookie !== "") headers["cookie"] = input.sessionCookie;
  const init: RequestInit = { method: input.method ?? "GET", headers, redirect: "manual" };
  if (init.method === "POST") {
    headers["content-type"] = "application/x-www-form-urlencoded";
    init.body = new URLSearchParams({ code: "parity-code", state: "parity-state" });
  }
  const res = await fetch(url, init);
  return { status: res.status, body: (await res.text()).slice(0, 2000) };
}

/**
 * True when the session behind `sessionCookie` carries the desktop client
 * marker (`pidash_client == "desktop"` in the decoded session). The marker
 * is set only by the desktop sign-in flow; browser sessions never carry it.
 */
export async function serverDeskSigninSessionHasDesktopMarker(sessionCookie: string): Promise<boolean> {
  const key = /(?:^|;\s*)session-id=([^;]+)/.exec(sessionCookie)?.[1] ?? "";
  if (key === "") throw new Error("[parity] session cookie carries no session-id.");
  const out = await desktopRuntimeShell(
    `import json\n` +
      `from pi_dash.db.models.session import Session\n` +
      `row = Session.objects.filter(session_key=${JSON.stringify(key)}).first()\n` +
      `data = row.get_decoded() if row is not None else {}\n` +
      `print("PARITY_DESK_SIGNIN:" + json.dumps({"desktop": data.get("pidash_client") == "desktop"}))\n`
  );
  const line = /^PARITY_DESK_SIGNIN:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error("[parity] desktop-marker probe produced no row.");
  return (JSON.parse(line) as { desktop?: unknown }).desktop === true;
}

// --- Pages navigation pane + versions (NEWFRONT-317, PAGE-045-054).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract. The pane reads live editor state, so most assertions are
// --- UI-side; these cover the version timeline's server state and the
// --- description payloads the overlay and restore write.

/** Project MEMBER role id (sibling of PROJECT_ROLE_GUEST above). */
export const PROJECT_ROLE_MEMBER = 15;

/** One page-version timeline row as the list endpoint returns it. */
export interface ParityPageVersion {
  id: string;
  lastSavedAt: string;
  ownedBy: string;
}

/** One page-version snapshot with both content payloads. */
export interface ParityPageVersionDetail extends ParityPageVersion {
  html: string;
  jsonText: string | null;
}

/** Live page description binary (Yjs state stream, latin1-decoded for substring reads). */
export interface ParityPageDescription {
  byteLength: number;
  text: string;
}

/** Live page record identity and actor facts. */
export interface ParityPageRecord {
  id: string;
  name: string;
  access: number | null;
  ownedBy: string;
  /** Last updater id, or null on a page nobody has touched yet. */
  updatedBy: string | null;
  createdAt: string;
  updatedAt: string;
  descriptionHtml: string;
  isLocked: boolean;
  /** Parent page id when the nested surface (PDASHOSS01-270) is live. */
  parentId?: string | null;
  /** Visible child count when the nested surface (PDASHOSS01-270) is live. */
  subPagesCount?: number;
  /** Shelved timestamp, when the record carries one. */
  archivedAt?: string | null;
}

/** Read a page's live description binary (Yjs state stream). */
export async function serverPageDescription(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPageDescription> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/description/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] page description read failed with HTTP ${res.status}.`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  return { byteLength: bytes.length, text: new TextDecoder("latin1").decode(bytes) };
}

/**
 * Overwrite a page's live description. Each HTML change queues the
 * server-side version snapshot, so version fixtures stage through here
 * with distinct authors (same-author snapshots coalesce server-side).
 */
export async function serverUpdatePageDescription(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  html: string | null,
  jsonText: string | null = null,
  binaryBase64: string | null = null,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const body: Record<string, unknown> = {};
  if (html !== null) body["description_html"] = html;
  if (jsonText !== null) body["description_json"] = JSON.parse(jsonText) as unknown;
  if (binaryBase64 !== null) body["description_binary"] = binaryBase64;
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/description/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(body),
    }
  );
  if (!res.ok) throw new Error(`[parity] page description write failed with HTTP ${res.status}: ${await res.text()}.`);
}

/** List a page's persisted version snapshots, oldest first as returned. */
export async function serverPageVersions(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPageVersion[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/versions/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] page versions read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["last_saved_at"] !== "string")
      throw new Error("[parity] version row carried no string id/last_saved_at.");
    return {
      id: rec["id"],
      lastSavedAt: rec["last_saved_at"],
      ownedBy: typeof rec["owned_by"] === "string" ? rec["owned_by"] : "",
    };
  });
}

/** Read one version snapshot with both content payloads. */
export async function serverPageVersionDetail(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  versionId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPageVersionDetail> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/versions/${versionId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] page version read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["last_saved_at"] !== "string")
    throw new Error("[parity] version detail carried no string id/last_saved_at.");
  return {
    id: rec["id"],
    lastSavedAt: rec["last_saved_at"],
    ownedBy: typeof rec["owned_by"] === "string" ? rec["owned_by"] : "",
    html: typeof rec["description_html"] === "string" ? rec["description_html"] : "",
    jsonText:
      rec["description_json"] === null || rec["description_json"] === undefined
        ? null
        : JSON.stringify(rec["description_json"]),
  };
}

/** Read a page's record identity and actor facts. */
export async function serverPageRecord(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityPageRecord> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  for (const key of ["id", "owned_by", "created_at", "updated_at"]) {
    if (typeof rec[key] !== "string") throw new Error(`[parity] page record carried no string ${key}.`);
  }
  return {
    id: rec["id"] as string,
    name: typeof rec["name"] === "string" ? rec["name"] : "",
    access: typeof rec["access"] === "number" ? rec["access"] : null,
    ownedBy: rec["owned_by"] as string,
    updatedBy: typeof rec["updated_by"] === "string" ? rec["updated_by"] : null,
    createdAt: rec["created_at"] as string,
    updatedAt: rec["updated_at"] as string,
    descriptionHtml: typeof rec["description_html"] === "string" ? rec["description_html"] : "",
    isLocked: rec["is_locked"] === true,
    ...(rec["parent_id"] === undefined
      ? {}
      : { parentId: typeof rec["parent_id"] === "string" ? rec["parent_id"] : null }),
    ...(typeof rec["sub_pages_count"] === "number" ? { subPagesCount: rec["sub_pages_count"] } : {}),
    ...(rec["archived_at"] === undefined
      ? {}
      : { archivedAt: typeof rec["archived_at"] === "string" ? rec["archived_at"] : null }),
  };
}

/** Lock a page (NEWFRONT-317 read-only fixtures stage the lock before opening). */
export async function serverPageLock(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/lock/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: "{}",
  });
  if (!res.ok) throw new Error(`[parity] page lock failed with HTTP ${res.status}.`);
}

/**
 * Ensure the workspace user with `email` holds the MEMBER project role.
 * Same shape as serverEnsureProjectGuest above, for the read-only-viewer
 * fixtures (a non-owner member on a private page cannot edit).
 */
export async function serverEnsureProjectMember(
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
    body: JSON.stringify({ members: [{ member_id: userId, role: PROJECT_ROLE_MEMBER }] }),
  });
  if (!res.ok) throw new Error(`[parity] project member ensure failed with HTTP ${res.status}.`);
}

// --- Estimates screen shell (NEWFRONT-321, EST-001-003, EST-026-028).
// --- Appended; existing helpers above are untouched per the shared
// --- harness contract.
// --- Estimates permissions matrix + edition split (NEWFRONT-326, EST-024
// --- + EST-029). serverEstimatesList is reused from the shell child
// --- (NEWFRONT-321, merged); serverEstimateRaw mirrors the edit
// --- sibling's probe (NEWFRONT-324, unmerged). Dedup at rebase,
// --- keeping one.

/** One estimate system as the project list reports it. */
export interface ParityServerEstimateSystem {
  id: string;
  name: string;
  lastUsed: boolean;
  points: ParityServerEstimatePoint[];
}

/** All estimate systems on a project, in API order. */
export async function serverEstimatesList(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerEstimateSystem[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] estimates read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
      throw new Error("[parity] estimate row carried no string id/name.");
    const points = Array.isArray(rec["points"]) ? rec["points"] : [];
    return {
      id: rec["id"],
      name: rec["name"],
      lastUsed: rec["last_used"] === true,
      points: points.map((point) => {
        const record = point as Record<string, unknown>;
        if (typeof record["id"] !== "string" || typeof record["value"] !== "string")
          throw new Error("[parity] estimate point carried no string id/value.");
        return { id: record["id"], value: record["value"] };
      }),
    };
  });
}

// --- Pages create, star, row actions, move (NEWFRONT-313, PAGE-007-017). ---
// Appended; existing helpers above are untouched per the shared driver contract.
// The favorite-pages pair below is the shared helper the header sibling
// (PAGE-056) reuses. (NEWFRONT-317 owns serverPageRecord/serverPageLock;
// this child reads the same endpoint through PagesRowPageFlags instead,
// because fresh pages carry updated_by null, which that record requires.)

/** Row-action page state the list behaviors hinge on (NEWFRONT-313). */
export interface PagesRowPageFlags {
  name: string;
  access: number | null;
  isFavorite: boolean;
  isLocked: boolean;
  archivedAt: string | null;
  markInUse: string | null;
}

/** Read one page's row-action state; throws naming the failure. */
export async function serverPagesRowFlags(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesRowPageFlags> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page flags read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const logo = rec["logo_props"] as Record<string, unknown> | null | undefined;
  return {
    name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
    access: typeof rec["access"] === "number" ? rec["access"] : null,
    isFavorite: rec["is_favorite"] === true,
    isLocked: rec["is_locked"] === true,
    archivedAt: typeof rec["archived_at"] === "string" ? rec["archived_at"] : null,
    markInUse:
      logo !== null && typeof logo === "object" && typeof logo["in_use"] === "string"
        ? (logo["in_use"] as string)
        : null,
  };
}

/** One row of the project pages list (NEWFRONT-313). */
export interface PagesRowListEntry {
  id: string;
  name: string;
  access: number | null;
  isFavorite: boolean;
  isLocked: boolean;
  archivedAt: string | null;
}

/** List a project's pages; throws naming the failure. */
export async function serverPagesRowList(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesRowListEntry[]> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] pages list failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    return {
      id: typeof rec["id"] === "string" ? (rec["id"] as string) : "",
      name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
      access: typeof rec["access"] === "number" ? rec["access"] : null,
      isFavorite: rec["is_favorite"] === true,
      isLocked: rec["is_locked"] === true,
      archivedAt: typeof rec["archived_at"] === "string" ? rec["archived_at"] : null,
    };
  });
}

// Estimates consumers (NEWFRONT-325, EST-018). Appended; existing helpers
// above are untouched per the shared parity contract.
/**
 * Clear a project's estimate link through the API (estimates off). This is
 * the same project-flag write the settings enable switch performs when it
 * is flipped off (pinned by EST-004); EST-018 asserts the gating outcome.
 */
export async function serverClearProjectEstimate(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    { estimate: null }
  );
  if (!res.ok) throw new Error(`[parity] project estimate clear failed with HTTP ${res.status}.`);
}

/** The estimate system id a project currently points at, or null when estimates are off. */
export async function serverProjectEstimate(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { estimate?: unknown };
  if (record.estimate === null || record.estimate === undefined) return null;
  if (typeof record.estimate !== "string") throw new Error("[parity] project estimate carried no string id.");
  return record.estimate;
}

// Modules cross-cutting parity helpers (NEWFRONT-311, rows MOD-036..043).
//
// Added additively to the harness (never altering the helpers above). The
// cross-cutting scenarios flip the project's modules feature flag through
// the settings UI and read the flag back here, resolve single modules for
// deep-link and no-live-update assertions, and read a work item's module
// membership for palette-picker preselection agreement. Fixture creation
// reuses the merged helpers (serverCreateModule, archivesPatchModule,
// serverProjectModules, serverCreateIssue, serverAddIssuesToModule).
// ---------------------------------------------------------------------------

/** One module's identity facts the cross-cutting scenarios assert on. */
export interface ModulesXModuleFacts {
  id: string;
  name: string;
  status: string;
}

/** Flip the project's modules feature flag (the settings toggle's PATCH). */
export async function modulesXSetModuleView(
  workspaceSlug: string,
  projectId: string,
  enabled: boolean,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    { module_view: enabled }
  );
  if (!res.ok) throw new Error(`[parity] module_view patch failed with HTTP ${res.status}.`);
}

/** One module's live facts (name + lifecycle status) by id. */
export async function modulesXModuleDetail(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ModulesXModuleFacts> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] module read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string" || typeof rec["status"] !== "string")
    throw new Error("[parity] module row carried no string id/name/status.");
  return { id: rec["id"] as string, name: rec["name"] as string, status: rec["status"] as string };
}

/** Module ids a work item currently belongs to (palette-picker agreement). */
export async function modulesXIssueModuleIds(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] issue read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const ids = rec["module_ids"];
  if (!Array.isArray(ids) || !ids.every((id) => typeof id === "string"))
    throw new Error("[parity] issue row carried no string-array module_ids.");
  return ids as string[];
}

// --- Modules dialogs/archived oracles (NEWFRONT-309, MOD-022..028).
// --- Appended; existing helpers above are untouched per the shared
// --- contract. Archive/restore writes reuse the merged archives*
// --- helpers read-only (live paths proved by NEWFRONT-225).

/** One module's full detail record (what the edit dialog prefills from). */
export interface ParityModuleDetail {
  id: string;
  name: string;
  description: string;
  status: string;
  leadId: string | null;
  memberIds: string[];
  startDate: string | null;
  targetDate: string | null;
  archivedAt: string | null;
}

/** Read one module's full record through the live detail endpoint. */
export async function modulesModuleDetail(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityModuleDetail> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    {
      headers: { cookie: sessionCookie },
    }
  );
  if (!res.ok) throw new Error(`[parity] module read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] module record carried no string id/name.");
  const rawMembers = rec["member_ids"];
  const memberIds = Array.isArray(rawMembers) ? rawMembers.filter((id): id is string => typeof id === "string") : [];
  return {
    id: rec["id"],
    name: rec["name"],
    description: typeof rec["description"] === "string" ? rec["description"] : "",
    status: typeof rec["status"] === "string" ? rec["status"] : "",
    leadId: typeof rec["lead_id"] === "string" ? rec["lead_id"] : null,
    memberIds,
    startDate: typeof rec["start_date"] === "string" ? rec["start_date"] : null,
    targetDate: typeof rec["target_date"] === "string" ? rec["target_date"] : null,
    archivedAt: typeof rec["archived_at"] === "string" ? rec["archived_at"] : null,
  };
}

/**
 * Whether a module is gone entirely: the live read 404s AND the archived
 * list carries no such row (a 404 alone also reads for archived modules).
 */
export async function modulesModuleGone(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    {
      headers: { cookie: sessionCookie },
    }
  );
  if (res.ok) return false;
  if (res.status !== 404) throw new Error(`[parity] module read failed with HTTP ${res.status}.`);
  const archived = await archivesArchivedModules(workspaceSlug, projectId, sessionCookie, apiBase);
  return !archived.some((row) => row.id === moduleId);
}

// --- Intake list content (NEWFRONT-256, INT-006/007/009/010/011). The merged
// --- serverCreateInboxIssue sends only a name; these helpers carry
// --- priority/labels/assignees at create time, intake PATCH (status/source),
// --- and param-aware list reads mirroring the UI's own query params, so
// --- scenarios can cross-check UI order/filter/paging against server state.

/** One intake list row as the list endpoint reports it. */
export interface ServerIntakeListRow {
  inboxId: string;
  issueId: string;
  status: number;
  source: string;
  name: string;
  sequenceId: number;
  priority: string | null;
  createdAt: string;
  createdBy: string;
  labelIds: string[];
}

function serverIntakeListRowOf(rec: Record<string, unknown>): ServerIntakeListRow {
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (typeof rec["id"] !== "string" || issue === undefined || typeof issue["id"] !== "string")
    throw new Error("[parity] intake list row carried no inbox/issue ids.");
  const labelIds = issue["label_ids"];
  return {
    inboxId: rec["id"] as string,
    issueId: issue["id"] as string,
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    source: typeof rec["source"] === "string" ? (rec["source"] as string) : "",
    name: typeof issue["name"] === "string" ? (issue["name"] as string) : "",
    sequenceId: typeof issue["sequence_id"] === "number" ? (issue["sequence_id"] as number) : 0,
    priority: typeof issue["priority"] === "string" ? (issue["priority"] as string) : null,
    createdAt: typeof issue["created_at"] === "string" ? (issue["created_at"] as string) : "",
    createdBy: typeof issue["created_by"] === "string" ? (issue["created_by"] as string) : "",
    labelIds: Array.isArray(labelIds) ? labelIds.map(String) : [],
  };
}

/** One intake list page: rows plus the cursor envelope. */
export interface ServerIntakeListPage {
  rows: ServerIntakeListRow[];
  nextCursor: string;
  nextPageResults: boolean;
  totalResults: number;
}

/** List intake rows with the UI's own query params (status, priority, labels, dates, order_by, cursor). */
export async function serverIntakeListPage(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  params: Record<string, string | number> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeListPage> {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) query.set(key, String(value));
  const suffix = query.size > 0 ? `?${query.toString()}` : "";
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${suffix}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] intake list read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const raw: unknown[] = Array.isArray(payload) ? payload : ((payload["results"] as unknown[]) ?? []);
  return {
    rows: raw.map((row) => serverIntakeListRowOf(row as Record<string, unknown>)),
    nextCursor: typeof payload["next_cursor"] === "string" ? (payload["next_cursor"] as string) : "",
    nextPageResults: payload["next_page_results"] === true,
    totalResults: typeof payload["total_results"] === "number" ? (payload["total_results"] as number) : 0,
  };
}

/** Every intake row matching `params`, following next_cursor (bounded at 20 pages). */
export async function serverIntakeListAll(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  params: Record<string, string | number> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeListRow[]> {
  const all: ServerIntakeListRow[] = [];
  let cursor = typeof params["cursor"] === "string" ? (params["cursor"] as string) : "";
  for (let page = 0; page < 20; page += 1) {
    const next: Record<string, string | number> = { ...params };
    if (cursor !== "") next["cursor"] = cursor;
    const fetched = await serverIntakeListPage(workspaceSlug, projectId, sessionCookie, next, apiBase);
    all.push(...fetched.rows);
    if (!fetched.nextPageResults || fetched.nextCursor === "") return all;
    cursor = fetched.nextCursor;
  }
  throw new Error("[parity] intake list read exceeded 20 pages.");
}

/** Rich intake detail (the list serializer omits updated_at/assignees/state). */
export interface ServerIntakeListDetail {
  status: number;
  source: string;
  name: string;
  sequenceId: number;
  priority: string | null;
  stateId: string;
  createdAt: string;
  updatedAt: string;
  createdBy: string;
  labelIds: string[];
  assigneeIds: string[];
}

/** One intake row by nested issue id, with dates/assignees/state for cross-checks. */
export async function serverIntakeListDetail(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerIntakeListDetail> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] intake detail read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const issue = rec["issue"] as Record<string, unknown> | undefined;
  if (issue === undefined) throw new Error("[parity] intake detail carried no nested issue.");
  const strArray = (value: unknown): string[] => (Array.isArray(value) ? value.map(String) : []);
  return {
    status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
    source: typeof rec["source"] === "string" ? (rec["source"] as string) : "",
    name: typeof issue["name"] === "string" ? (issue["name"] as string) : "",
    sequenceId: typeof issue["sequence_id"] === "number" ? (issue["sequence_id"] as number) : 0,
    priority: typeof issue["priority"] === "string" ? (issue["priority"] as string) : null,
    stateId: typeof issue["state_id"] === "string" ? (issue["state_id"] as string) : "",
    createdAt: typeof issue["created_at"] === "string" ? (issue["created_at"] as string) : "",
    updatedAt: typeof issue["updated_at"] === "string" ? (issue["updated_at"] as string) : "",
    createdBy: typeof issue["created_by"] === "string" ? (issue["created_by"] as string) : "",
    labelIds: strArray(issue["label_ids"]),
    assigneeIds: strArray(issue["assignee_ids"]),
  };
}

/** Options for rich intake-row creation (beyond the merged name-only helper). */
export interface ServerIntakeListCreateOptions {
  priority?: string;
  labelIds?: string[];
  assigneeIds?: string[];
  /** FORMS/EMAIL rows: create ignores it server-side, so it is applied via admin PATCH after. */
  source?: string;
}

/** Create an intake row carrying priority/labels/assignees (and optionally a non-default source). */
export async function serverIntakeListCreate(
  workspaceSlug: string,
  projectId: string,
  name: string,
  sessionCookie: string,
  opts: ServerIntakeListCreateOptions = {},
  apiBase: string = apiBaseFromEnv()
): Promise<{ inboxId: string; issueId: string }> {
  const issue: Record<string, unknown> = { name };
  if (opts.priority !== undefined) issue["priority"] = opts.priority;
  if (opts.labelIds !== undefined) issue["label_ids"] = opts.labelIds;
  if (opts.assigneeIds !== undefined) issue["assignee_ids"] = opts.assigneeIds;
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/`,
    sessionCookie,
    { source: "IN_APP", issue }
  );
  if (!res.ok) throw new Error(`[parity] intake list create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const nested = rec["issue"] as Record<string, unknown> | undefined;
  if (typeof rec["id"] !== "string" || typeof nested?.["id"] !== "string")
    throw new Error("[parity] created intake row carried no inbox/issue ids.");
  const created = { inboxId: rec["id"] as string, issueId: nested["id"] as string };
  if (opts.source !== undefined && opts.source !== "IN_APP") {
    await serverIntakeListPatch(
      workspaceSlug,
      projectId,
      created.issueId,
      { source: opts.source },
      sessionCookie,
      apiBase
    );
  }
  return created;
}

/** Intake PATCH payload: top-level intake fields plus nested issue fields. */
export interface ServerIntakeListPatchData {
  status?: number;
  source?: string;
  snoozedTill?: string | null;
  duplicateTo?: string;
  issue?: Record<string, unknown>;
}

/** PATCH an intake row by nested issue id (admin session for status/source changes). */
export async function serverIntakeListPatch(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  data: ServerIntakeListPatchData,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const body: Record<string, unknown> = {};
  if (data.status !== undefined) body["status"] = data.status;
  if (data.source !== undefined) body["source"] = data.source;
  if (data.snoozedTill !== undefined) body["snoozed_till"] = data.snoozedTill;
  if (data.duplicateTo !== undefined) body["duplicate_to"] = data.duplicateTo;
  if (data.issue !== undefined) body["issue"] = data.issue;
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    body
  );
  if (!res.ok) throw new Error(`[parity] intake list patch failed with HTTP ${res.status}.`);
}

/** Workspace member id for an email (project adds address members, not users). */
export async function serverWorkspaceMemberId(
  workspaceSlug: string,
  email: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace member list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ member?: { id?: unknown; email?: unknown } }>;
  const id = rows.find((row) => row.member?.email === email)?.member?.id;
  if (typeof id !== "string") throw new Error(`[parity] workspace user ${email} not found.`);
  return id;
}

/** The project's stored IANA timezone (cycle date chips render its offset). */
export async function serverProjectTimezone(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { timezone?: unknown };
  if (typeof record.timezone !== "string") throw new Error("[parity] project carried no string timezone.");
  return record.timezone;
}

/** Raw estimate-endpoint probe: status plus leniently parsed body (never throws on status). */
export interface ParityEstimateRawProbe {
  status: number;
  json: unknown;
}

/**
 * Raw request against an estimate path (rule-matrix probes assert the
 * status themselves). `path` is the URL path below the API base; the
 * body parses as JSON when it parses at all, else stays null.
 */
export async function serverEstimateRaw(
  method: "GET" | "POST" | "PATCH" | "DELETE" | "PUT",
  path: string,
  sessionCookie: string,
  body?: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  const csrf = csrfFromCookie(sessionCookie);
  const res = await fetch(`${apiBase}${path}`, {
    method,
    headers: {
      cookie: sessionCookie,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...(csrf === "" ? {} : { "X-CSRFToken": csrf, referer: `${apiBase}${path}` }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json: unknown = await res.json().catch(() => null);
  return { status: res.status, json };
}

// --- Pages list discovery (NEWFRONT-312, PAGE-001-006).

/** One project page as the list endpoint reports it. */
export interface PagesListServerRow {
  id: string;
  name: string;
  /** 0 public, 1 private. */
  access: number;
  ownedBy: string;
  isFavorite: boolean;
  archivedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

/** List a project's pages (live rows; archived excluded unless requested). */
export async function serverListPages(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  includeArchived = false,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesListServerRow[]> {
  const params = includeArchived ? "?include_archived=true" : "";
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${params}`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] page list failed with HTTP ${res.status}.`);
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows.map((row) => {
    const rec = row as Record<string, unknown>;
    if (typeof rec["id"] !== "string") throw new Error("[parity] page list carried a row without a string id.");
    return {
      id: rec["id"] as string,
      name: typeof rec["name"] === "string" ? (rec["name"] as string) : "",
      access: typeof rec["access"] === "number" ? (rec["access"] as number) : 0,
      ownedBy: typeof rec["owned_by"] === "string" ? (rec["owned_by"] as string) : "",
      isFavorite: rec["is_favorite"] === true,
      archivedAt: typeof rec["archived_at"] === "string" ? (rec["archived_at"] as string) : null,
      createdAt: typeof rec["created_at"] === "string" ? (rec["created_at"] as string) : "",
      updatedAt: typeof rec["updated_at"] === "string" ? (rec["updated_at"] as string) : "",
    };
  });
}

/** Set a page's access level (0 public, 1 private); owner-only on the server. */
export async function serverSetPageAccess(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  access: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`,
    sessionCookie,
    { access },
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] page access patch failed with HTTP ${res.status}.`);
}

/** Rename one page (also bumps its modification date for sort fixtures). */
export async function serverRenamePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`,
    sessionCookie,
    { name },
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(
      `[parity] page rename failed with HTTP ${res.status}: ${JSON.stringify(res.payload).slice(0, 300)}.`
    );
  }
}

/** Read a project's pages feature switch (the PAGE-001 gate flag). */
export async function serverProjectPageView(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  return (res.payload as { page_view?: unknown }).page_view === true;
}

/** Archive one page without deleting it (the shelved-tab fixture step). */
export async function serverArchivePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`,
    sessionCookie,
    {},
    apiBase
  );
  if (res.status !== 200 && res.status !== 201 && res.status !== 204) {
    throw new Error(`[parity] page archive failed with HTTP ${res.status}.`);
  }
}

/** Favorite one page (shared user-favorites surface; PAGE-009/PAGE-056 reuse it). */
export async function serverFavoritePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  name: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await createFavorite(
    workspaceSlug,
    sessionCookie,
    { entity_type: "page", entity_identifier: pageId, project_id: projectId, entity_data: { name } },
    apiBase
  );
}

/** Remove one page's favorite mark (no-op when it carries none). */
export async function serverUnfavoritePage(
  workspaceSlug: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const rows = await listFavorites(workspaceSlug, sessionCookie, apiBase);
  const match = rows.find((row) => row.entity_type === "page" && row.entity_identifier === pageId);
  if (match !== undefined) await deleteFavorite(workspaceSlug, sessionCookie, match.id, apiBase);
}

// --- Pages gating, list states, URLs, detail shell (NEWFRONT-314,
// --- PAGE-018-025,079-080). Appended; existing helpers above are untouched
// --- per the shared parity contract. Restore, unlock, and delete round out
// --- the gating fixtures; the status probe pins server-vs-client divergence
// --- for the PAGE-018 permission matrix without throwing on refusal.

/** Restore one shelved page (DELETE on the archive endpoint). */
export async function serverUnarchivePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] page unarchive failed with HTTP ${res.status}.`);
  }
}

/** Unlock one page (DELETE on the lock endpoint). */
export async function serverPageUnlock(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/lock/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] page unlock failed with HTTP ${res.status}.`);
  }
}

/**
 * Non-throwing page-request probe for the permission matrix: answers the
 * HTTP status so scenarios can pin where the server refuses what the UI
 * hides (open confirmation #6). `body` stays undefined for bodyless verbs.
 */
export async function serverPageRequestStatus(
  method: "GET" | "POST" | "PATCH" | "PUT" | "DELETE",
  path: string,
  sessionCookie: string,
  body?: unknown,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await apiJson(method, path, sessionCookie, body, apiBase);
  return res.status;
}

// --- Fresh-workspace seed settling (NEWFRONT-265). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. POST /api/workspaces/ unconditionally queues the workspace_seed
// celery task, which creates exactly one project NAMED AFTER THE WORKSPACE
// seconds to minutes later (the seed file's "Pi Dash Demo Project" title is
// discarded; only its description and demo content survive). Specs that
// provision a fresh workspace and then assert emptiness or exact counts race
// that task: on a fast worker the seed lands before the assertion and fails
// it, on a slow worker the same spec passes by luck. These helpers make
// setup deterministic in both directions: wait for the seed where the
// scenario needs it (the home dashboard renders its body only once seeded),
// and drain it where the scenario asserts emptiness or exact counts. The
// drain deletes through the Django shell because the API refuses to delete
// the workspace's default project, and the seed project is the default
// whenever it lands first.

/** Polling budget for seed waits. */
export interface SeedSettleOptions {
  /** Total time to wait for the seed project to appear. Defaults to 300_000. */
  timeoutMs?: number;
  /** Delay between project-list polls. Defaults to 2000. */
  pollMs?: number;
  /** Extra cleanup passes after the first delete. Defaults to 4. */
  settleRounds?: number;
  /** Delay between cleanup passes. Defaults to 5000. */
  settleMs?: number;
}

/**
 * Whether a project row is the backend's seed for a workspace. The match is
 * exact-name equality: scenario projects must never reuse the workspace
 * name, or the drain cannot tell them apart.
 */
export function isWorkspaceSeedProject(projectName: string, workspaceName: string): boolean {
  return projectName === workspaceName;
}

/**
 * Django snippet removing one workspace's seed rows, mirroring the app's own
 * project delete: the seed project by id is soft-deleted (list endpoints
 * exclude soft-deleted rows, and the delete queues the related-objects
 * cascade), and so are the bot's workspace-level pages and membership,
 * while the bot user — which has no soft-delete — is hard-deleted (its
 * membership cascades with it). Soft (not hard) deleting the project keeps
 * a mid-flight seed task harmless to assertions: the seed task has no
 * celery retry, so if the drain lands mid-flight the task fails once on
 * the deleted bot's foreign keys while its partial rows stay hidden with
 * the soft-deleted project. Idempotent: a second run deletes nothing and
 * prints zeros. Built by a function so the unit test can pin the escaping
 * and the printed marker without a stack.
 */
export function workspaceSeedCleanupPython(slug: string, projectId: string): string {
  return (
    `from django.db.models import Q\n` +
    `from pi_dash.db.models import Page, Project, User, Workspace, WorkspaceMember\n` +
    `ws = Workspace.objects.get(slug=${JSON.stringify(slug)})\n` +
    `proj = Project.objects.filter(id=${JSON.stringify(projectId)}, workspace=ws).first()\n` +
    `projects = 0\n` +
    `if proj is not None:\n` +
    `    proj.delete()\n` +
    `    projects = 1\n` +
    `bot = User.objects.filter(username="bot_user_" + str(ws.id)).first()\n` +
    `pages, members, users = 0, 0, 0\n` +
    `if bot is not None:\n` +
    `    pages = Page.objects.filter(workspace=ws).filter(Q(created_by=bot) | Q(owned_by=bot)).delete()\n` +
    `    members = WorkspaceMember.objects.filter(workspace=ws, member=bot).delete()\n` +
    `    users, _ = User.objects.filter(id=bot.id).delete()\n` +
    `import json as parity_json\n` +
    `print("PARITY_SEED_DRAIN:" + parity_json.dumps({"projects": projects, "pages": pages, "members": members, "users": users}))\n`
  );
}

/**
 * Counts from one drain snippet's marker row. Exported for the unit test;
 * the drain loop below is its only production caller.
 */
export function seedDrainOf(out: string): { projects: number; pages: number; members: number; users: number } {
  const line = /^PARITY_SEED_DRAIN:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error("[parity] seed drain produced no marker row.");
  const row = JSON.parse(line) as Record<string, unknown>;
  for (const key of ["projects", "pages", "members", "users"]) {
    if (typeof row[key] !== "number") throw new Error("[parity] seed drain marker carried no numeric counts.");
  }
  return row as { projects: number; pages: number; members: number; users: number };
}

/** Lists a workspace's projects as id/name pairs for the seed wait. */
type SeedProjectLister = () => Promise<Array<{ id: string; name: string }>>;

/** Project id/name pairs through a raw session cookie (no AuthedSession). */
async function cookieProjectList(
  sessionCookie: string,
  apiBase: string,
  slug: string
): Promise<Array<{ id: string; name: string }>> {
  const res = await fetch(`${apiBase}/api/workspaces/${slug}/projects/details/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] projects/details read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map((r) => {
    if (typeof r["id"] !== "string" || typeof r["name"] !== "string") {
      throw new Error("[parity] project row carried no string id and name.");
    }
    return { id: r["id"], name: r["name"] };
  });
}

async function awaitSeededProjectId(
  list: SeedProjectLister,
  slug: string,
  workspaceName: string,
  opts: SeedSettleOptions
): Promise<string> {
  const timeoutMs = opts.timeoutMs ?? 300_000;
  const pollMs = opts.pollMs ?? 2000;
  const started = Date.now();
  for (;;) {
    const found = (await list()).find((p) => isWorkspaceSeedProject(p.name, workspaceName));
    if (found !== undefined) return found.id;
    if (Date.now() - started >= timeoutMs) {
      throw new Error(
        `[parity] seed project for workspace ${slug} never appeared within ${timeoutMs}ms; ` +
          `is the stack worker running?`
      );
    }
    await sleep(pollMs);
  }
}

async function drainSeededProject(
  list: SeedProjectLister,
  slug: string,
  workspaceName: string,
  opts: SeedSettleOptions
): Promise<void> {
  const settleRounds = opts.settleRounds ?? 4;
  const settleMs = opts.settleMs ?? 5000;
  const projectId = await awaitSeededProjectId(list, slug, workspaceName, opts);
  for (let round = 0; round <= settleRounds; round += 1) {
    if (round > 0) await sleep(settleMs);
    const counts = seedDrainOf(await apiShell(workspaceSeedCleanupPython(slug, projectId)));
    if (counts.projects + counts.pages + counts.members + counts.users === 0) break;
    if (round === settleRounds) {
      throw new Error(`[parity] seed drain for workspace ${slug} kept deleting rows after ${settleRounds + 1} passes.`);
    }
  }
  const leftover = (await list()).find((p) => isWorkspaceSeedProject(p.name, workspaceName));
  if (leftover !== undefined) {
    throw new Error(`[parity] seed project for workspace ${slug} is still listed after the drain.`);
  }
}

/**
 * Wait until the backend's seed project appears in a fresh workspace and
 * return its id. For scenarios that need the seed to render (home
 * dashboard). Throws on timeout: with no seed the workspace never reaches
 * the state the scenario asserts, so a loud failure beats a flaky pass —
 * check the stack worker first.
 */
export async function awaitWorkspaceSeededProject(
  session: AuthedSession,
  slug: string,
  workspaceName: string,
  opts: SeedSettleOptions = {}
): Promise<string> {
  return awaitSeededProjectId(() => projectsDetails(session, slug), slug, workspaceName, opts);
}

/**
 * Cookie-session variant of awaitWorkspaceSeededProject, for callers that
 * authenticate with a raw session cookie instead of an AuthedSession.
 */
export async function awaitWorkspaceSeededProjectWithSession(
  sessionCookie: string,
  slug: string,
  workspaceName: string,
  apiBase: string = apiBaseFromEnv(),
  opts: SeedSettleOptions = {}
): Promise<string> {
  return awaitSeededProjectId(() => cookieProjectList(sessionCookie, apiBase, slug), slug, workspaceName, opts);
}

/**
 * Remove a fresh workspace's backend seed so emptiness and exact-count
 * assertions hold regardless of worker speed. Waits for the seed project to
 * appear, soft-deletes it plus the bot rows through the Django shell (the
 * API refuses to delete the default project at all), then re-runs the
 * cleanup while anything is still being deleted, catching rows that
 * committed inside an earlier pass's race window (once the bot user is
 * gone no further seed writes can succeed, so the passes converge).
 * Finishes by proving over the API that no seed-named project remains
 * listed.
 */
export async function drainWorkspaceSeed(
  session: AuthedSession,
  slug: string,
  workspaceName: string,
  opts: SeedSettleOptions = {}
): Promise<void> {
  return drainSeededProject(() => projectsDetails(session, slug), slug, workspaceName, opts);
}

/**
 * Cookie-session variant of drainWorkspaceSeed, for callers that
 * authenticate with a raw session cookie instead of an AuthedSession.
 */
export async function drainWorkspaceSeedWithSession(
  sessionCookie: string,
  slug: string,
  workspaceName: string,
  apiBase: string = apiBaseFromEnv(),
  opts: SeedSettleOptions = {}
): Promise<void> {
  return drainSeededProject(() => cookieProjectList(sessionCookie, apiBase, slug), slug, workspaceName, opts);
}

// --- Modules list frame and header controls (NEWFRONT-306, MOD-001..007).
// --- Appended; existing helpers above are untouched per the shared contract.

/** Favorite one module for the session user (favorites-filter fixture setup). */
export async function modulesFavoriteModule(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/user-favorite-modules/`,
    sessionCookie,
    { module: moduleId }
  );
  if (!res.ok) throw new Error(`[parity] module favorite failed with HTTP ${res.status}.`);
}

/** Switch one page's visibility; access 0 is open, 1 is restricted. */
export async function serverPageAccess(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  access: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/access/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ access }),
  });
  if (!res.ok) throw new Error(`[parity] page access failed with HTTP ${res.status}.`);
}

/** Visibility-switch status without throwing (negative-path pin). */
export async function serverPageAccessStatus(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  access: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/access/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ access }),
  });
  return res.status;
}

/** Shelve one page; resolves with the archived stamp. */
export async function serverPageArchive(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page archive failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { archived_at?: unknown };
  return typeof payload.archived_at === "string" ? payload.archived_at : "";
}

/** Restore one shelved page. */
export async function serverPageRestore(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page restore failed with HTTP ${res.status}.`);
}

/** Duplicate one page; resolves with the copy's id and name. */
export async function serverDuplicatePage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; name: string }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/duplicate/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie },
    }
  );
  if (!res.ok) throw new Error(`[parity] page duplicate failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { id?: unknown; name?: unknown };
  if (typeof payload.id !== "string") throw new Error("[parity] page duplicate returned no id.");
  return { id: payload.id, name: typeof payload.name === "string" ? payload.name : "" };
}

/** Let guests view all project features (guest list would otherwise be empty). */
export async function serverPagesRowEnableGuestViewAll(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ guest_view_all_features: true }),
  });
  if (!res.ok) throw new Error(`[parity] project guest-view patch failed with HTTP ${res.status}.`);
}

/** Move-attempt status without throwing (the community tree carries no move route). */
export async function serverMovePageStatus(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  newProjectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/move/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ new_project_id: newProjectId }),
  });
  return res.status;
}

// --- Estimates enable switch, active/archived display, row gating
// --- (NEWFRONT-322, EST-004-007). Appended; existing helpers above are
// --- untouched per the shared harness contract. The system list reader
// --- is the shell child's serverEstimatesList (NEWFRONT-321, merged).
// --- Estimates saved-system edit/delete, whole-system delete, server
// --- rules (NEWFRONT-324, EST-014-016 + EST-025). serverEstimatesList
// --- and serverEstimateRaw are reused from the merged shell/permissions
// --- siblings (NEWFRONT-321/326); the helpers below are this child's
// --- additive probes, kept whole at the rebase.

/** The estimate system a project currently links (null when estimates are off). */
export async function serverProjectEstimateId(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (rec["estimate"] === null || rec["estimate"] === undefined) return null;
  if (typeof rec["estimate"] !== "string") throw new Error("[parity] project estimate link is not a string id.");
  return rec["estimate"];
}

/**
 * Create a dormant (archived-side) estimate system through the API: the
 * stock creator always marks systems in-use, while the API default leaves
 * them dormant, which is what the archived section lists.
 */
export async function serverCreateArchivedEstimate(
  workspaceSlug: string,
  projectId: string,
  name: string,
  points: string[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; lastUsed: boolean; points: ParityServerEstimatePoint[] }> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`,
    sessionCookie,
    {
      estimate: { name, type: "points", last_used: false },
      estimate_points: points.map((value) => ({ value })),
    }
  );
  if (!res.ok) throw new Error(`[parity] archived estimate create failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] created estimate carried no string id.");
  const rows = Array.isArray(rec["points"]) ? rec["points"] : [];
  return {
    id: rec["id"],
    lastUsed: rec["last_used"] === true,
    points: rows.map((row) => {
      const point = row as Record<string, unknown>;
      if (typeof point["id"] !== "string" || typeof point["value"] !== "string")
        throw new Error("[parity] estimate point carried no string id/value.");
      return { id: point["id"], value: point["value"] };
    }),
  };
}

/** One estimate point with its position key (renumber assertions). */
export interface ParityServerEstimatePointKeyed {
  id: string;
  key: number;
  value: string;
}

/** One estimate system with positioned points, as the retrieve endpoint reports it. */
export interface ParityServerEstimateDetail {
  id: string;
  name: string;
  type: string;
  lastUsed: boolean;
  points: ParityServerEstimatePointKeyed[];
}

/** Single estimate system read (null when the id is gone). Distinct from the creation child's throwing serverEstimateDetail (NEWFRONT-323): delete-path assertions need the null, not an exception. */
export async function serverEstimateDetailOrNull(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerEstimateDetail | null> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`[parity] estimate read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] estimate detail carried no string id/name.");
  const points = Array.isArray(rec["points"]) ? rec["points"] : [];
  return {
    id: rec["id"],
    name: rec["name"],
    type: typeof rec["type"] === "string" ? rec["type"] : "",
    lastUsed: rec["last_used"] === true,
    points: points.map((point) => {
      const record = point as Record<string, unknown>;
      if (typeof record["id"] !== "string" || typeof record["key"] !== "number" || typeof record["value"] !== "string")
        throw new Error("[parity] estimate point carried no id/key/value.");
      return { id: record["id"], key: record["key"], value: record["value"] };
    }),
  };
}

/** Module work-item counters as the modules list read reports them (NEWFRONT-307, MOD-012/020). */
export interface ModulesListProgress {
  completed: number;
  cancelled: number;
  total: number;
}

/**
 * One module's counters from the project modules read. Specs poll this
 * until issue adds and state moves land server-side before entering the
 * UI (the list never refreshes on its own).
 */
export async function modulesListProgress(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ModulesListProgress> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] modules read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  for (const row of rows) {
    const rec = row as Record<string, unknown>;
    if (rec["id"] !== moduleId) continue;
    const completed = rec["completed_issues"];
    const cancelled = rec["cancelled_issues"];
    const total = rec["total_issues"];
    if (typeof completed !== "number" || typeof cancelled !== "number" || typeof total !== "number") {
      throw new Error("[parity] module row carried no numeric counters.");
    }
    return { completed, cancelled, total };
  }
  throw new Error("[parity] module missing from its project list read.");
}

/** One estimate system with typed, position-keyed values as the server reports it (NEWFRONT-323). Distinct from the shell child's list shape (NEWFRONT-321, ParityServerEstimateSystem): the creation dialog's renumber assertions need the system type and point keys. */
export interface ParityServerEstimateSystemKeyed {
  id: string;
  name: string;
  type: string;
  last_used: boolean;
  points: { id: string; key: number; value: string }[];
}

/** Parse one estimate record; shared by the list and detail readers. */
function parseServerEstimateSystemKeyed(rec: Record<string, unknown>): ParityServerEstimateSystemKeyed {
  if (
    typeof rec["id"] !== "string" ||
    typeof rec["name"] !== "string" ||
    typeof rec["type"] !== "string" ||
    typeof rec["last_used"] !== "boolean"
  )
    throw new Error("[parity] estimate carried no id/name/type/last_used.");
  const rows = Array.isArray(rec["points"]) ? rec["points"] : [];
  return {
    id: rec["id"],
    name: rec["name"],
    type: rec["type"],
    last_used: rec["last_used"],
    points: rows.map((row) => {
      const point = row as Record<string, unknown>;
      if (typeof point["id"] !== "string" || typeof point["key"] !== "number" || typeof point["value"] !== "string")
        throw new Error("[parity] estimate point carried no string id/value or numeric key.");
      return { id: point["id"], key: point["key"], value: point["value"] };
    }),
  };
}

/** List a project's estimate systems through the API (NEWFRONT-323). */
export async function serverListEstimates(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerEstimateSystemKeyed[]> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] estimate list failed with HTTP ${res.status}.`);
  const body = (await res.json()) as unknown[];
  return body.map((row) => parseServerEstimateSystemKeyed(row as Record<string, unknown>));
}

/** Read one estimate system through the API (NEWFRONT-323). */
export async function serverEstimateDetail(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityServerEstimateSystemKeyed> {
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] estimate detail failed with HTTP ${res.status}.`);
  return parseServerEstimateSystemKeyed((await res.json()) as Record<string, unknown>);
}

/** PATCH a saved system (name/type plus the full point set); status asserted by the caller. */
export async function serverPatchEstimateSystem(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  payload: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/`,
    sessionCookie,
    payload,
    apiBase
  );
}

/** DELETE a whole estimate system; status asserted by the caller. */
export async function serverDeleteEstimateSystem(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
}

/** DELETE a single point, optionally moving affected items to a replacement; status by caller. */
export async function serverDeleteEstimatePoint(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  pointId: string,
  sessionCookie: string,
  replacementId?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "DELETE",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/estimate-points/${pointId}/`,
    sessionCookie,
    replacementId === undefined ? undefined : { new_estimate_id: replacementId },
    apiBase
  );
}

/** POST a single point; status asserted by the caller (EST-025 required-field matrix). */
export async function serverCreateEstimatePoint(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  payload: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/estimate-points/`,
    sessionCookie,
    payload,
    apiBase
  );
}

/** PATCH a single point; status asserted by the caller (EST-025 length-cap path). */
export async function serverPatchEstimatePoint(
  workspaceSlug: string,
  projectId: string,
  estimateId: string,
  pointId: string,
  payload: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/${estimateId}/estimate-points/${pointId}/`,
    sessionCookie,
    payload,
    apiBase
  );
}

/** Bulk-create a system with its points; status asserted by the caller (EST-025 matrix). */
export async function serverCreateEstimateRaw(
  workspaceSlug: string,
  projectId: string,
  payload: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityEstimateRawProbe> {
  return serverEstimateRaw(
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`,
    sessionCookie,
    payload,
    apiBase
  );
}

// --- Module detail server reads (NEWFRONT-310, MOD-029-035). Appended;
// --- existing helpers above are untouched per the shared harness
// --- contract. The detail page, its analytics sidebar, progress
// --- breakdowns, and links assert against these: the full module record,
// --- a PATCH for fixtures and inline-edit proofs, the caller's
// --- per-module preferences, and the record's embedded links (the API
// --- serves no standalone links list; the client reads link_module).

/** One module's detail record as the server reports it (shape-tolerant). */
export interface ServerModulesDetailRecord {
  id: string;
  name: string;
  description: string;
  status: string;
  startDate: string | null;
  targetDate: string | null;
  leadId: string | null;
  memberIds: string[];
  totalIssues: number;
  completedIssues: number;
  totalPoints: number;
  completedPoints: number;
  archivedAt: string | null;
  linkModule: Array<Record<string, unknown>>;
}

/** Read one module's full detail record. */
export async function serverModulesDetailGet(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerModulesDetailRecord> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] module detail read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["name"] !== "string")
    throw new Error("[parity] module record carried no string id/name.");
  const num = (value: unknown): number => (typeof value === "number" ? value : 0);
  const date = (value: unknown): string | null => (typeof value === "string" ? value : null);
  const members = Array.isArray(rec["member_ids"])
    ? (rec["member_ids"] as unknown[]).filter((id): id is string => typeof id === "string")
    : [];
  return {
    id: rec["id"],
    name: rec["name"],
    description: typeof rec["description"] === "string" ? rec["description"] : "",
    status: typeof rec["status"] === "string" ? rec["status"] : "",
    startDate: date(rec["start_date"]),
    targetDate: date(rec["target_date"]),
    leadId: typeof rec["lead_id"] === "string" ? rec["lead_id"] : null,
    memberIds: members,
    totalIssues: num(rec["total_issues"]),
    completedIssues: num(rec["completed_issues"]),
    totalPoints: num(rec["total_estimate_points"]),
    completedPoints: num(rec["completed_estimate_points"]),
    archivedAt: date(rec["archived_at"]),
    linkModule: Array.isArray(rec["link_module"]) ? (rec["link_module"] as Array<Record<string, unknown>>) : [],
  };
}

/** PATCH one module's definition (dates, status, lead, members for fixtures). */
export async function serverModulesDetailPatch(
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
  if (!res.ok) throw new Error(`[parity] module detail patch failed with HTTP ${res.status}.`);
}

/** The caller's per-module preferences (stored layout plus filter state). */
export interface ServerModulesDetailUserProperties {
  layout: string | null;
  richFilters: unknown;
  filters: Record<string, unknown>;
}

/** Read the caller's per-module preferences. */
export async function serverModulesDetailUserProperties(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerModulesDetailUserProperties> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/modules/${moduleId}/user-properties/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] module user-properties read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const display = rec["display_filters"] as Record<string, unknown> | undefined;
  return {
    layout: typeof display?.["layout"] === "string" ? (display["layout"] as string) : null,
    richFilters: rec["rich_filters"] ?? null,
    filters: (rec["filters"] as Record<string, unknown> | undefined) ?? {},
  };
}

/** One module link as the detail record embeds it. */
export interface ServerModulesDetailLink {
  id: string;
  title: string;
  url: string;
  createdBy: string;
  createdAt: string;
}

/** Links currently attached to a module (from its detail record). */
export async function serverModulesDetailLinks(
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerModulesDetailLink[]> {
  const detail = await serverModulesDetailGet(workspaceSlug, projectId, moduleId, sessionCookie, apiBase);
  return detail.linkModule.map((row) => ({
    id: typeof row["id"] === "string" ? row["id"] : "",
    title: typeof row["title"] === "string" ? row["title"] : "",
    url: typeof row["url"] === "string" ? row["url"] : "",
    createdBy: typeof row["created_by"] === "string" ? row["created_by"] : "",
    createdAt: typeof row["created_at"] === "string" ? row["created_at"] : "",
  }));
}

// ---------------------------------------------------------------------------
// Pages detail header actions (NEWFRONT-318, PAGE-055-064 + PAGE-074).
// Distinctive pagesHeader* prefix; sibling ranges own pagesLive* (319),
// pagesPane* (317), pagesPrefs*/pagesKeys*/pagesAssist*/pagesCloud*/pagesDesk*/
// pagesNeg* (320). The detail star writes the generic user-favorites row while
// the row names the page-scoped endpoint — both converge on the same
// UserFavorite row (page.is_favorite is an Exists over it), so scenarios
// assert the record flag plus the user-favorites list. Note: the backend
// routes only the favorite-pages detail (POST/DELETE); the collection GET
// some rows name answers 404, so no list helper exists.
// ---------------------------------------------------------------------------

/** A project page record with the header-asserted fields. */
export interface PagesHeaderPageRecord {
  id: string;
  name: string;
  /** 0 = open, 1 = restricted (EPageAccess). */
  access: number;
  is_favorite: boolean;
  is_locked: boolean;
  archived_at: string | null;
  owned_by: string;
  description_html: string | null;
}

/** Read one project page; throws naming the failure. */
export async function pagesHeaderReadPage(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesHeaderPageRecord> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as {
    id?: unknown;
    name?: unknown;
    access?: unknown;
    is_favorite?: unknown;
    is_locked?: unknown;
    archived_at?: unknown;
    owned_by?: unknown;
    description_html?: unknown;
  };
  if (typeof row.id !== "string" || typeof row.name !== "string" || typeof row.owned_by !== "string") {
    throw new Error("[parity] page read carried no string id/name/owned_by.");
  }
  return {
    id: row.id,
    name: row.name,
    access: typeof row.access === "number" ? row.access : -1,
    is_favorite: row.is_favorite === true,
    is_locked: row.is_locked === true,
    archived_at: typeof row.archived_at === "string" ? row.archived_at : null,
    owned_by: row.owned_by,
    description_html: typeof row.description_html === "string" ? row.description_html : null,
  };
}

/** List a project's pages (open + restricted; shelved live under their own read). */
export async function pagesHeaderListPages(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesHeaderPageRecord[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page list failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { results?: unknown };
  const rows = Array.isArray(payload.results) ? payload.results : Array.isArray(payload) ? payload : [];
  return (rows as Array<Record<string, unknown>>)
    .filter((row) => typeof row["id"] === "string")
    .map((row) => ({
      id: row["id"] as string,
      name: typeof row["name"] === "string" ? row["name"] : "",
      access: typeof row["access"] === "number" ? row["access"] : -1,
      is_favorite: row["is_favorite"] === true,
      is_locked: row["is_locked"] === true,
      archived_at: typeof row["archived_at"] === "string" ? row["archived_at"] : null,
      owned_by: typeof row["owned_by"] === "string" ? row["owned_by"] : "",
      description_html: typeof row["description_html"] === "string" ? row["description_html"] : null,
    }));
}

/** Favorite or unfavorite a page through the page-scoped endpoint; throws naming the failure. */
export async function pagesHeaderSetFavorite(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  favorite: boolean,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    favorite ? "POST" : "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/favorite-pages/${pageId}/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] favorite patch failed with HTTP ${res.status}.`);
}

/** Switch a page between open (0) and restricted (1); throws naming the failure. */
export async function pagesHeaderSetAccess(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  access: 0 | 1,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "POST",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/access/`,
    sessionCookie,
    { access }
  );
  if (!res.ok) throw new Error(`[parity] access patch failed with HTTP ${res.status}.`);
}

/** Shelve or restore a page; throws naming the failure. */
export async function pagesHeaderSetArchive(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  archived: boolean,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    archived ? "POST" : "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/archive/`,
    sessionCookie
  );
  if (!res.ok) throw new Error(`[parity] archive patch failed with HTTP ${res.status}.`);
}

/** Generic user-favorite entities of one type (the detail star writes here). */
export async function pagesHeaderUserFavoriteEntities(
  workspaceSlug: string,
  entityType: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Array<{ entity_identifier: string; entity_type: string }>> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/user-favorites/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] user-favorites read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { results?: unknown };
  const rows = Array.isArray(payload.results) ? payload.results : Array.isArray(payload) ? payload : [];
  return (rows as Array<Record<string, unknown>>)
    .filter((row) => row["entity_type"] === entityType && typeof row["entity_identifier"] === "string")
    .map((row) => ({ entity_identifier: row["entity_identifier"] as string, entity_type: entityType }));
}

// --- Pages title, mark + formatting toolbar (NEWFRONT-315, PAGE-026-036).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract. Distinctive pagesTitle*/pagesToolbar* names stay clear of
// --- the sibling serverPage* helpers (NEWFRONT-317/320).

/** A project page record as the detail endpoint reports it. */
export interface PagesTitleRecord {
  id: string;
  name: string;
  logoProps: unknown;
  isLocked: boolean;
  access: number | null;
  descriptionHtml: string;
}

/** Read one project page; throws naming the failure. */
export async function pagesTitleReadRecord(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PagesTitleRecord> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] page read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") throw new Error("[parity] page record carried no string id.");
  return {
    id: rec["id"],
    name: typeof rec["name"] === "string" ? rec["name"] : "",
    logoProps: rec["logo_props"] ?? null,
    isLocked: rec["is_locked"] === true,
    access: typeof rec["access"] === "number" ? rec["access"] : null,
    descriptionHtml: typeof rec["description_html"] === "string" ? rec["description_html"] : "",
  };
}

/** PATCH a page record (title/mark fixtures); throws naming the failure. */
export async function pagesTitlePatchRecord(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok)
    throw new Error(`[parity] page patch failed with HTTP ${res.status}: ${(await res.text()).slice(0, 300)}.`);
}

/**
 * PATCH a page record without throwing, for refusal assertions (the status
 * IS the expectation). Resolves with status + body.
 */
export async function pagesTitlePatchRecordLenient(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; bodyText: string }> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: res.status, bodyText: (await res.text()).slice(0, 500) };
}

/** Overwrite a page's live description (body fixtures). */
export async function pagesToolbarWriteDescription(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  html: string | null,
  jsonText: string | null = null,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const body: Record<string, unknown> = {};
  if (html !== null) body["description_html"] = html;
  if (jsonText !== null) body["description_json"] = JSON.parse(jsonText) as unknown;
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/description/`,
    {
      method: "PATCH",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify(body),
    }
  );
  if (!res.ok) throw new Error(`[parity] page description write failed with HTTP ${res.status}: ${await res.text()}.`);
}

/**
 * Poll the record until description_html contains `needle` (the fallback
 * save path is async). Resolves with the final html; throws on timeout.
 */
export async function pagesToolbarPollDescriptionHtml(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  needle: string,
  timeoutMs = 180_000,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const started = Date.now();
  for (;;) {
    const record = await pagesTitleReadRecord(workspaceSlug, projectId, pageId, sessionCookie, apiBase);
    if (record.descriptionHtml.includes(needle)) return record.descriptionHtml;
    if (Date.now() - started > timeoutMs) {
      throw new Error(
        `[parity] description never contained ${JSON.stringify(needle)} (len ${record.descriptionHtml.length}).`
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Poll the record until name equals `want`; resolves with the name. */
export async function pagesTitlePollName(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  sessionCookie: string,
  want: string,
  timeoutMs = 60_000,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const started = Date.now();
  for (;;) {
    const record = await pagesTitleReadRecord(workspaceSlug, projectId, pageId, sessionCookie, apiBase);
    if (record.name === want) return record.name;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] page name stuck at ${JSON.stringify(record.name)}, wanted ${JSON.stringify(want)}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
}

/** File-asset bytes behind an editor image (upload-registration asserts). */
export interface PagesToolbarAssetBytes {
  status: number;
  byteLength: number;
  contentType: string;
}

/**
 * Follow an editor image src (the asset endpoint redirects to presigned
 * object bytes) and report what arrived. The first hop needs the session;
 * the signed redirect carries its own auth.
 */
export async function pagesToolbarAssetBytes(
  assetSrc: string,
  sessionCookie: string,
  oracleBase: string = pagesLiveOracleBase()
): Promise<PagesToolbarAssetBytes> {
  const url = assetSrc.startsWith("http") ? assetSrc : `${oracleBase}${assetSrc.startsWith("/") ? "" : "/"}${assetSrc}`;
  const res = await fetchTolerant(url, { headers: { cookie: sessionCookie } });
  const buffer = Buffer.from(await res.arrayBuffer());
  return {
    status: res.status,
    byteLength: buffer.length,
    contentType: res.headers.get("content-type") ?? "",
  };
}
// --- Runs/approvals oracle fixtures (NEWFRONT-180). Appended; existing
// --- helpers above are untouched per the shared harness contract.
// ---
// --- The web API can create a run (queued), cancel it, decide an approval
// --- and re-tick an issue — but no web endpoint sets a run's status, posts
// --- run events, files approval requests, or exhausts a ticker budget
// --- (those are runner-daemon or scheduler writes). Scenarios that need
// --- those states build scenario-owned rows through the stack's Django
// --- shell instead, and delete them afterward. The container name honors
// --- PARITY_CONTAINER_PREFIX so slot-private stacks work.

/** The stack's api container, honoring slot-private prefixes. */
function runsApiContainer(): string {
  const prefix = (process.env["PARITY_CONTAINER_PREFIX"] ?? "parity19").trim() || "parity19";
  return `${prefix}-api`;
}

/** Run a Django shell snippet inside the stack's api container; resolves with stdout. */
export async function runsShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", runsApiContainer(), "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/** Workspace + user ids backing the seed slug/email, for direct run creation. */
export async function runsLookupIds(
  workspaceSlug: string,
  email: string
): Promise<{ workspaceId: string; userId: string }> {
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `print("PARITY_RUN_IDS:" + json.dumps({"workspace": str(ws.id), "user": str(user.id)}))\n`
  );
  const line = /^PARITY_RUN_IDS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const parsed = (line === "" ? null : (JSON.parse(line) as { workspace?: unknown; user?: unknown })) ?? null;
  if (parsed === null || typeof parsed.workspace !== "string" || typeof parsed.user !== "string") {
    throw new Error("[parity] runs id lookup returned no workspace/user ids.");
  }
  return { workspaceId: parsed.workspace, userId: parsed.user };
}

/** One page of the caller's runs, newest first. */
export async function runsListPage(
  sessionCookie: string,
  options: { page?: number; perPage?: number; workspaceId?: string; projectId?: string } = {},
  apiBase: string = apiBaseFromEnv()
): Promise<{ results: Record<string, unknown>[]; total_count: number; total_pages: number; page: number }> {
  const params = new URLSearchParams();
  params.set("page", String(options.page ?? 1));
  if (options.perPage !== undefined) params.set("per_page", String(options.perPage));
  if (options.workspaceId !== undefined) params.set("workspace", options.workspaceId);
  if (options.projectId !== undefined) params.set("project", options.projectId);
  const res = await authedApi(`/runners/runs/?${params.toString()}`, sessionCookie, undefined, apiBase);
  requireOk(res, "runs list");
  const payload = (await res.json()) as {
    results?: unknown[];
    total_count?: unknown;
    total_pages?: unknown;
    page?: unknown;
  };
  if (
    !Array.isArray(payload.results) ||
    typeof payload.total_count !== "number" ||
    typeof payload.total_pages !== "number" ||
    typeof payload.page !== "number"
  ) {
    throw new Error("[parity] runs list envelope carried no results/counts.");
  }
  return {
    results: payload.results as Record<string, unknown>[],
    total_count: payload.total_count,
    total_pages: payload.total_pages,
    page: payload.page,
  };
}

/** One run by id, optionally with its events (detail serializer). */
export async function runsGet(
  runId: string,
  sessionCookie: string,
  includeEvents = false,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const suffix = includeEvents ? "?include_events=1" : "";
  const res = await authedApi(`/runners/runs/${runId}/${suffix}`, sessionCookie, undefined, apiBase);
  requireOk(res, "run read");
  return (await res.json()) as Record<string, unknown>;
}

/** Create a run through the direct (prompt-body) web path; resolves with the created row. */
export async function runsCreateDirect(
  sessionCookie: string,
  input: { workspace: string; prompt: string; pod?: string; work_item?: string },
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authedApi(
    `/runners/runs/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    },
    apiBase
  );
  requireOk(res, "run create");
  return (await res.json()) as Record<string, unknown>;
}

/** Cancel a run through the web path; resolves with the updated row. */
export async function runsCancel(
  runId: string,
  sessionCookie: string,
  reason = "parity oracle",
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authedApi(
    `/runners/runs/${runId}/cancel/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ reason }),
    },
    apiBase
  );
  requireOk(res, "run cancel");
  return (await res.json()) as Record<string, unknown>;
}

/** Pending approvals routed to the caller, optionally project-scoped. */
export async function runsApprovalsList(
  sessionCookie: string,
  projectId?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>[]> {
  const suffix = projectId !== undefined ? `?project=${projectId}` : "";
  const res = await authedApi(`/runners/approvals/${suffix}`, sessionCookie, undefined, apiBase);
  return collectionRows(res, "approvals read");
}

/** Decide one approval; resolves with the HTTP status plus the row when accepted. */
export async function runsApprovalDecide(
  approvalId: string,
  decision: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; record: Record<string, unknown> | null }> {
  const res = await authedApi(
    `/runners/approvals/${approvalId}/decide/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ decision }),
    },
    apiBase
  );
  if (!res.ok) return { status: res.status, record: null };
  return { status: res.status, record: (await res.json()) as Record<string, unknown> };
}

/** Re-tick an issue's agent-run budget; granted/refuse both resolve (never throw). */
export async function runsReTick(
  workItemId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await authedApi(
    `/runners/re-tick/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ work_item: workItemId }),
    },
    apiBase
  );
  requireOk(res, "re-tick");
  return (await res.json()) as Record<string, unknown>;
}

/** The issue's execution-target pin (`agent_executor`), null when it inherits the project default. */
export async function runsIssueExecutor(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const record = await fetchIssue(workspaceSlug, projectId, issueId, sessionCookie, apiBase);
  const value = record["agent_executor"];
  if (value !== null && typeof value !== "string") throw new Error("[parity] issue row carried no agent_executor.");
  return value;
}

/** The project's default execution target, null when the serializer omits it. */
export async function runsProjectDefaultExecutor(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string | null> {
  const { record } = await projectFacts(workspaceSlug, projectId, sessionCookie, apiBase);
  const value = record["default_agent_executor"];
  if (value === undefined || value === null) return null;
  if (typeof value !== "string") throw new Error("[parity] project row carried a non-string executor default.");
  return value;
}

/** One run-event fixture: kind plus payload (seq assigned in order). */
export interface RunsEventFixture {
  kind: string;
  payload?: Record<string, unknown>;
}

/** One tool-call fixture for a cloud run's inspection list. */
export interface RunsToolCallFixture {
  tool_name: string;
  risk?: string;
  status?: string;
}

/** One approval-request fixture on a run. */
export interface RunsApprovalFixture {
  kind?: string;
  payload?: Record<string, unknown>;
  reason?: string;
  expires_in_seconds?: number | null;
}

/** One scenario-owned run row, with optional children. */
export interface RunsRunFixture {
  prompt: string;
  status?: string;
  executor?: string;
  work_item?: string | null;
  scheduler_binding?: string | null;
  error?: string;
  done_payload?: Record<string, unknown> | null;
  tool_plan?: Record<string, unknown> | null;
  events?: RunsEventFixture[];
  tool_calls?: RunsToolCallFixture[];
  approvals?: RunsApprovalFixture[];
}

/**
 * Create scenario-owned run rows (plus events, tool calls, approvals) in one
 * shell call. `podId` anchors every row's project. Resolves with the created
 * run ids in input order. Callers delete them with {@link runsDeleteRuns}.
 */
export async function runsCreateFixtures(
  workspaceSlug: string,
  email: string,
  podId: string,
  runs: RunsRunFixture[]
): Promise<string[]> {
  const spec = JSON.stringify(runs);
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import AgentRun, AgentRunEvent, AgentRunToolCall, ApprovalRequest, Pod\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `pod = Pod.objects.get(pk=${JSON.stringify(podId)})\n` +
      `made = []\n` +
      `for index, item in enumerate(json.loads(${JSON.stringify(spec)})):\n` +
      `    run = AgentRun.objects.create(workspace=ws, created_by=user, pod=pod,\n` +
      `        prompt=item.get("prompt", ""), status=item.get("status", "queued"),\n` +
      `        executor_kind=item.get("executor", "local_runner"),\n` +
      `        work_item_id=item.get("work_item"), scheduler_binding_id=item.get("scheduler_binding"),\n` +
      `        error=item.get("error", ""),\n` +
      `        done_payload=item.get("done_payload"), tool_plan=item.get("tool_plan") or {})\n` +
      `    for seq, event in enumerate(item.get("events") or [], start=1):\n` +
      `        AgentRunEvent.objects.create(agent_run=run, seq=seq, kind=event.get("kind", ""),\n` +
      `            payload=event.get("payload") or {})\n` +
      `    for position, call in enumerate(item.get("tool_calls") or []):\n` +
      `        AgentRunToolCall.objects.create(agent_run=run,\n` +
      `            tool_call_id=f"parity-{run.id}-{position}", source="internal",\n` +
      `            tool_name=call.get("tool_name", ""), risk=call.get("risk", "read"),\n` +
      `            status=call.get("status", "prepared"), request_fingerprint="parity")\n` +
      `    for approval in item.get("approvals") or []:\n` +
      `        expires = approval.get("expires_in_seconds")\n` +
      `        from datetime import timedelta\n` +
      `        from django.utils import timezone\n` +
      `        AgentRunApproval = ApprovalRequest\n` +
      `        AgentRunApproval.objects.create(agent_run=run, kind=approval.get("kind", "other"),\n` +
      `            payload=approval.get("payload") or {}, reason=approval.get("reason", ""),\n` +
      `            expires_at=(timezone.now() + timedelta(seconds=expires)) if expires else None)\n` +
      `    made.append(str(run.id))\n` +
      `print("PARITY_RUN_MADE:" + json.dumps(made))\n`
  );
  const line = /^PARITY_RUN_MADE:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const ids = (line === "" ? null : (JSON.parse(line) as unknown)) as string[] | null;
  if (!Array.isArray(ids) || ids.length !== runs.length || ids.some((id) => typeof id !== "string")) {
    throw new Error("[parity] run fixture create returned no ids.");
  }
  return ids;
}

/** Read one approval's stored decision state (status/source/decider email). */
export async function runsApprovalStatus(approvalId: string): Promise<{
  status: string;
  decision_source: string;
  decided_by: string | null;
}> {
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.runner.models import ApprovalRequest\n` +
      `approval = ApprovalRequest.objects.select_related("decided_by").get(pk=${JSON.stringify(approvalId)})\n` +
      `print("PARITY_APPROVAL:" + json.dumps({"status": approval.status,\n` +
      `    "decision_source": approval.decision_source,\n` +
      `    "decided_by": approval.decided_by.email if approval.decided_by_id else None}))\n`
  );
  const line = /^PARITY_APPROVAL:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const parsed =
    (line === ""
      ? null
      : (JSON.parse(line) as { status?: unknown; decision_source?: unknown; decided_by?: unknown })) ?? null;
  if (parsed === null || typeof parsed.status !== "string" || typeof parsed.decision_source !== "string") {
    throw new Error("[parity] approval read returned no decision state.");
  }
  return {
    status: parsed.status,
    decision_source: parsed.decision_source,
    decided_by: typeof parsed.decided_by === "string" ? parsed.decided_by : null,
  };
}

/** Delete scenario-owned runs (events, tool calls and approvals cascade). Best effort per row. */
export async function runsDeleteRuns(runIds: string[]): Promise<void> {
  if (runIds.length === 0) return;
  await runsShell(
    `from pi_dash.runner.models import AgentRun\n` +
      `AgentRun.objects.filter(pk__in=${JSON.stringify(runIds)}).delete()\n` +
      `print("PARITY_RUN_DELETED")\n`
  );
}

/** Move one run to another status mid-test (poll-stop proofs); resolves with the stored status. */
export async function runsSetStatus(runId: string, status: string): Promise<string> {
  const out = await runsShell(
    `from pi_dash.runner.models import AgentRun\n` +
      `run = AgentRun.objects.get(pk=${JSON.stringify(runId)})\n` +
      `run.status = ${JSON.stringify(status)}\n` +
      `run.save(update_fields=["status"])\n` +
      `print("PARITY_RUN_STATUS:" + run.status)\n`
  );
  const stored = /^PARITY_RUN_STATUS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (stored === "") throw new Error("[parity] run status write returned nothing.");
  return stored;
}

/** Read one run's stored status (cancel/decision server-state checks). */
export async function runsStoredStatus(runId: string): Promise<string> {
  const out = await runsShell(
    `from pi_dash.runner.models import AgentRun\n` +
      `print("PARITY_RUN_STATUS:" + AgentRun.objects.get(pk=${JSON.stringify(runId)}).status)\n`
  );
  const stored = /^PARITY_RUN_STATUS:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (stored === "") throw new Error("[parity] run status read returned nothing.");
  return stored;
}

/**
 * Exhaust an issue's ticker budget (used := cap) so re-tick has something to
 * grant. The ticker row must already exist (the issue entered a ticking state
 * through the API first). Resolves with used/cap after the write.
 */
export async function runsExhaustTicker(issueId: string): Promise<{ used: number; cap: number }> {
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker\n` +
      // Rows are created lazily by the scheduling machinery; mirror its
      // disabled-unarmed defaults so a fresh issue's row looks produced.
      `ticker, _ = IssueAgentTicker.objects.get_or_create(issue_id=${JSON.stringify(issueId)}, defaults={\n` +
      `    "user_disabled": False, "next_run_at": None, "used": 0, "granted": 0,\n` +
      `    "enabled": False, "disarm_reason": ""})\n` +
      `cap = ticker.effective_max_ticks()\n` +
      `ticker.used = cap\n` +
      `ticker.save(update_fields=["used"])\n` +
      `print("PARITY_TICKER:" + json.dumps({"used": ticker.used, "cap": cap}))\n`
  );
  const line = /^PARITY_TICKER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const parsed = (line === "" ? null : (JSON.parse(line) as { used?: unknown; cap?: unknown })) ?? null;
  if (parsed === null || typeof parsed.used !== "number" || typeof parsed.cap !== "number") {
    throw new Error("[parity] ticker exhaust returned no used/cap.");
  }
  return { used: parsed.used, cap: parsed.cap };
}

/**
 * Create a scenario-owned scheduler definition plus one project binding.
 * Resolves with the binding id and scheduler name for run-detail link
 * assertions. Callers delete both with {@link runsDeleteScheduler}.
 */
export async function runsCreateScheduler(
  workspaceSlug: string,
  projectId: string,
  email: string,
  slug: string,
  name: string
): Promise<{ schedulerId: string; bindingId: string; schedulerName: string }> {
  const out = await runsShell(
    `import json\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import Project, User, Workspace\n` +
      `from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `project = Project.objects.get(pk=${JSON.stringify(projectId)})\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `scheduler = Scheduler.objects.create(workspace=ws, slug=${JSON.stringify(slug)},\n` +
      `    name=${JSON.stringify(name)}, prompt="parity scheduler prompt")\n` +
      `binding = SchedulerBinding.objects.create(project=project, scheduler=scheduler,\n` +
      `    dtstart=timezone.now(), actor=user)\n` +
      `print("PARITY_SCHED:" + json.dumps({"scheduler": str(scheduler.id),\n` +
      `    "binding": str(binding.id), "name": scheduler.name}))\n`
  );
  const line = /^PARITY_SCHED:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const parsed =
    (line === "" ? null : (JSON.parse(line) as { scheduler?: unknown; binding?: unknown; name?: unknown })) ?? null;
  if (
    parsed === null ||
    typeof parsed.scheduler !== "string" ||
    typeof parsed.binding !== "string" ||
    typeof parsed.name !== "string"
  ) {
    throw new Error("[parity] scheduler create returned no ids.");
  }
  return { schedulerId: parsed.scheduler, bindingId: parsed.binding, schedulerName: parsed.name };
}

/** Delete a scenario-owned scheduler (its binding cascades). */
export async function runsDeleteScheduler(schedulerId: string): Promise<void> {
  await runsShell(
    `from pi_dash.db.models.scheduler import Scheduler\n` +
      `Scheduler.objects.filter(pk=${JSON.stringify(schedulerId)}).delete()\n` +
      `print("PARITY_SCHED_DELETED")\n`
  );
}

/** Read an issue's ticker budget (used/granted/cap) for grant assertions. */
export async function runsTickerBudget(issueId: string): Promise<{ used: number; granted: number; cap: number }> {
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker\n` +
      `ticker = IssueAgentTicker.objects.get(issue_id=${JSON.stringify(issueId)})\n` +
      `print("PARITY_TICKER:" + json.dumps({"used": ticker.used, "granted": ticker.granted,\n` +
      `    "cap": ticker.effective_max_ticks()}))\n`
  );
  const line = /^PARITY_TICKER:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const parsed =
    (line === "" ? null : (JSON.parse(line) as { used?: unknown; granted?: unknown; cap?: unknown })) ?? null;
  if (
    parsed === null ||
    typeof parsed.used !== "number" ||
    typeof parsed.granted !== "number" ||
    typeof parsed.cap !== "number"
  ) {
    throw new Error("[parity] ticker read returned no budget.");
  }
  return { used: parsed.used, granted: parsed.granted, cap: parsed.cap };
}

/**
 * Register a scenario-owned runner (offline manual enrollment, owned by the
 * seed user) in the project's default pod, so dispatch preflight finds
 * execution capacity. Offline is honest capacity: the matcher counts
 * registered non-revoked runners regardless of heartbeat, and the
 * dispatched run waits visibly in queued. Callers delete it afterward.
 */
export async function runsCreateRunner(
  workspaceSlug: string,
  email: string,
  projectId: string,
  name: string
): Promise<string> {
  const out = await runsShell(
    `import json\n` +
      `from pi_dash.db.models import User, Workspace\n` +
      `from pi_dash.runner.models import Pod, Runner\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `pod = Pod.objects.get(project_id=${JSON.stringify(projectId)}, is_default=True)\n` +
      `runner = Runner.objects.create(owner=user, workspace=ws, pod=pod,\n` +
      `    name=${JSON.stringify(name)})\n` +
      `print("PARITY_RUNNER_MADE:" + json.dumps(str(runner.id)))\n`
  );
  const line = /^PARITY_RUNNER_MADE:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  const id = (line === "" ? null : (JSON.parse(line) as unknown)) as string | null;
  if (typeof id !== "string") {
    throw new Error("[parity] runner create returned no id.");
  }
  return id;
}

/** Delete a scenario-owned runner (best-effort: already-gone is fine). */
export async function runsDeleteRunner(runnerId: string): Promise<void> {
  await runsShell(
    `from pi_dash.runner.models import Runner\n` +
      `Runner.objects.filter(pk=${JSON.stringify(runnerId)}).delete()\n` +
      `print("PARITY_RUNNER_GONE:1")\n`
  );
}

// --- Analytics shell oracle reads (NEWFRONT-375, ANL-001..004 + ANL-018). ---
// Appended; existing helpers above are untouched per the shared contract.

/** One raw analytics read: status plus parsed payload (null when unparsable). */
export interface AnalyticsRawRead {
  status: number;
  payload: unknown;
}

/** Workspace display name read back from the API (ANL-002 title assertion). */
export async function analyticsWorkspaceName(session: AuthedSession, slug: string): Promise<string> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/`);
  if (!res.ok) throw new Error(`[parity] workspace read failed with HTTP ${res.status}.`);
  const ws = (await res.json()) as { name?: unknown };
  if (typeof ws.name !== "string") throw new Error("[parity] workspace read carried no string name.");
  return ws.name;
}

/**
 * Raw headline-totals read for one analytics tab, optionally narrowed to a
 * project subset (ANL-004 scoping, ANL-018 guest read). Mirrors the query the
 * overview/work-items sections send: `tab` always, `project_ids` only when
 * the picker holds a non-empty selection.
 */
export async function analyticsAdvanceTotalsRaw(
  session: AuthedSession,
  slug: string,
  tab: "overview" | "work-items",
  projectIds: string[] = []
): Promise<AnalyticsRawRead> {
  const params = new URLSearchParams({ tab });
  if (projectIds.length > 0) params.set("project_ids", projectIds.join(","));
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/advance-analytics?${params.toString()}`);
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw project-create status (ANL-003 member/guest negative, ANL-018). The
 * throwing create helper cannot express a refusal, so negatives read the
 * status directly: 200/201 for admins and members, a refusal for guests.
 */
export async function analyticsProjectCreateStatus(
  session: AuthedSession,
  slug: string,
  input: { name: string; identifier: string }
): Promise<number> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${slug}/projects/`, {
    name: input.name,
    identifier: input.identifier,
    network: NETWORK.PUBLIC,
  });
  return res.status;
}

// --- Workspace issue exports (NEWFRONT-381, EXP-001..013). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. Server-state reads plus Django-shell manufacturing for the
// exports-settings oracle scenarios. Real submissions still go through the
// UI against the live endpoint; the shell helpers below exist only for
// fixtures the UI cannot produce on demand: bulk history past one page,
// records older than seven days, stuck in-flight records (no background
// task behind them), and terminal states the happy path never reaches
// (failed). Every planted fixture is labeled at the scenario call site.

/** One export-history record as the list endpoint serializes it. */
export interface ExportsHistoryRecord {
  id: string;
  provider: string;
  status: string;
  url: string | null;
  project: string[];
  created_at: string;
  initiated_by_detail: {
    id: string;
    display_name: string;
    email: string;
    avatar_url: string | null;
  };
}

function exportsRecordOf(raw: unknown): ExportsHistoryRecord {
  const record = raw as Record<string, unknown>;
  const detail = (record["initiated_by_detail"] ?? {}) as Record<string, unknown>;
  const projects = Array.isArray(record["project"]) ? record["project"] : [];
  if (typeof record["id"] !== "string") throw new Error("[parity] export record carried no id.");
  return {
    id: record["id"],
    provider: typeof record["provider"] === "string" ? record["provider"] : "",
    status: typeof record["status"] === "string" ? record["status"] : "",
    url: typeof record["url"] === "string" ? record["url"] : null,
    project: projects.filter((p): p is string => typeof p === "string"),
    created_at: typeof record["created_at"] === "string" ? record["created_at"] : "",
    initiated_by_detail: {
      id: typeof detail["id"] === "string" ? detail["id"] : "",
      display_name: typeof detail["display_name"] === "string" ? detail["display_name"] : "",
      email: typeof detail["email"] === "string" ? detail["email"] : "",
      avatar_url: typeof detail["avatar_url"] === "string" ? detail["avatar_url"] : null,
    },
  };
}

/**
 * Paged history read mirroring the history section's query (the endpoint
 * requires both cursor and per_page). Returns the page plus its cursors.
 */
export async function exportsListHistory(
  session: AuthedSession,
  slug: string,
  cursor = "10:0:0",
  perPage = 10
): Promise<{
  results: ExportsHistoryRecord[];
  next_page_results: boolean;
  prev_page_results: boolean;
  next_cursor: string;
  prev_cursor: string;
}> {
  const params = new URLSearchParams({ cursor, per_page: String(perPage) });
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/export-issues/?${params.toString()}`);
  if (!res.ok) throw new Error(`[parity] export history list failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const results = Array.isArray(payload["results"]) ? payload["results"] : [];
  return {
    results: results.map(exportsRecordOf),
    next_page_results: payload["next_page_results"] === true,
    prev_page_results: payload["prev_page_results"] === true,
    next_cursor: typeof payload["next_cursor"] === "string" ? payload["next_cursor"] : "",
    prev_cursor: typeof payload["prev_cursor"] === "string" ? payload["prev_cursor"] : "",
  };
}

/**
 * Raw submit through the live export endpoint. Never throws on refusal so
 * negative paths can pin the status; the UI submit path is driven through
 * the browser instead.
 */
export async function exportsSubmitStatus(
  session: AuthedSession,
  slug: string,
  body: { provider: string; project: string[]; multiple: boolean }
): Promise<{ status: number; payload: unknown }> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${slug}/export-issues/`, body);
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** Find one record by id, scanning the first pages of history. */
export async function exportsFindRecord(
  session: AuthedSession,
  slug: string,
  id: string
): Promise<ExportsHistoryRecord | null> {
  let cursor = "10:0:0";
  for (let page = 0; page < 10; page += 1) {
    const list = await exportsListHistory(session, slug, cursor, 10);
    const found = list.results.find((r) => r.id === id);
    if (found) return found;
    if (!list.next_page_results || list.next_cursor === "") return null;
    cursor = list.next_cursor;
  }
  return null;
}

/** Django-shell container for export fixtures on this run's stack. */
function exportsApiContainer(): string {
  return (process.env["PARITY_API_CONTAINER"] ?? "parity19-api").trim() || "parity19-api";
}

/**
 * Plant one history record directly in the database (bulk-history,
 * aged-record, stuck-processing, and failed-state fixtures the UI cannot
 * produce on demand). Returns the record id. Callers label the planting.
 */
export async function exportsPlantRecord(input: {
  workspaceSlug: string;
  ownerEmail: string;
  provider?: string;
  projectIds?: string[];
  status?: string;
  createdDaysAgo?: number;
  url?: string | null;
}): Promise<string> {
  const provider = input.provider ?? "csv";
  const status = input.status ?? "completed";
  const projectIds = input.projectIds ?? [];
  const url = input.url ?? "https://example.invalid/parity-exports/planted.csv";
  const daysAgo = input.createdDaysAgo ?? 0;
  const out = await apiShellOn(
    exportsApiContainer(),
    `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import ExporterHistory, User, Workspace\n` +
      `user = User.objects.get(email=${JSON.stringify(input.ownerEmail)})\n` +
      `workspace = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `record = ExporterHistory.objects.create(workspace=workspace, project=${JSON.stringify(projectIds)}, initiated_by=user, provider=${JSON.stringify(provider)}, type="issue_exports", status="queued")\n` +
      `stamp = timezone.now() - timedelta(days=${JSON.stringify(daysAgo)})\n` +
      `ExporterHistory.objects.filter(id=record.id).update(status=${JSON.stringify(status)}, url=${JSON.stringify(url)}, created_at=stamp, updated_at=stamp)\n` +
      `print("PARITY_EXPORT_ID:" + str(record.id))\n`
  );
  const id = /^PARITY_EXPORT_ID:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error("[parity] export plant produced no record id.");
  return id;
}

/** Flip one record's status, age, and/or file link (settle a stuck poll, age a download). */
export async function exportsPatchRecord(
  recordId: string,
  patch: { status?: string; createdDaysAgo?: number; url?: string | null }
): Promise<void> {
  const sets: string[] = [];
  if (patch.status !== undefined) sets.push(`status=${JSON.stringify(patch.status)}`);
  if (patch.url !== undefined) sets.push(`url=${JSON.stringify(patch.url)}`);
  const stamp =
    patch.createdDaysAgo === undefined
      ? ""
      : `stamp = timezone.now() - timedelta(days=${JSON.stringify(patch.createdDaysAgo)})\n` +
        `ExporterHistory.objects.filter(id=record.id).update(created_at=stamp, updated_at=stamp)\n`;
  const out = await apiShellOn(
    exportsApiContainer(),
    `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import ExporterHistory\n` +
      `record = ExporterHistory.objects.get(id=${JSON.stringify(recordId)})\n` +
      (sets.length > 0 ? `ExporterHistory.objects.filter(id=record.id).update(${sets.join(", ")})\n` : ``) +
      stamp +
      `print("PARITY_EXPORT_PATCHED")\n`
  );
  if (!/^PARITY_EXPORT_PATCHED$/m.test(out)) throw new Error("[parity] export patch produced no confirmation.");
}

/**
 * Fetch one user's profile activity export exactly as the profile download
 * control does (NEWFRONT-382, EXP-022): the raw CSV text the server returns
 * for `date`, so the oracle scenario can prove the saved file carries the
 * server's bytes.
 */
export async function profileActivityExportCsv(
  session: AuthedSession,
  workspaceSlug: string,
  userId: string,
  date: string
): Promise<string> {
  const res = await sessionFetch(session, "POST", `/api/workspaces/${workspaceSlug}/user-activity/${userId}/export/`, {
    date,
  });
  if (!res.ok) throw new Error(`[parity] profile activity export failed with HTTP ${res.status}.`);
  return res.text();
}

/**
 * Raw issue-reaction POST status (NEWFRONT-116, CMT-024). The server refuses
 * a second identical add with a client error instead of duplicating the row,
 * so duplicate scenarios read the status directly: 201 for the first add, a
 * 4xx refusal carrying the error body for the repeat.
 */
export async function serverAddIssueReactionRaw(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  sessionCookie: string,
  reaction: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: string }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/reactions/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ reaction }),
    }
  );
  return { status: res.status, body: await res.text() };
}

/**
 * Raw comment-reaction POST status (NEWFRONT-116, CMT-024). Same contract as
 * the issue twin: 201 for the first add, a 4xx refusal for the repeat.
 */
export async function serverAddCommentReactionRaw(
  workspaceSlug: string,
  projectId: string,
  commentId: string,
  sessionCookie: string,
  reaction: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: string }> {
  const res = await fetch(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/comments/${commentId}/reactions/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({ reaction }),
    }
  );
  return { status: res.status, body: await res.text() };
}

// ---------------------------------------------------------------------------
// Project lifecycle oracles (NEWFRONT-390, rows PSET-013..017).
//
// Added additively to the shared helpers above (never forking them). The
// lifecycle scenarios drive the UI for what the user sees and read back
// through these helpers for what the server stored: the published-board
// record after publish/update/unpublish writes.
// ---------------------------------------------------------------------------

/** One published-board record as the deploy-boards read reports it. */
export interface PublishBoardFacts {
  id: string;
  anchor: string | null;
  is_comments_enabled: boolean;
  is_reactions_enabled: boolean;
  is_votes_enabled: boolean;
  view_props: Record<string, unknown>;
}

/**
 * Delete a scenario-created project that carries the workspace-default flag.
 * The first project of a drained workspace is the default, which the API
 * refuses to delete: hand the flag to a sacrificial project first (its
 * PATCH clears every other default atomically), then delete the
 * scenario-created project. The spare lives on in the scenario-owned
 * workspace, invisible to every other scenario.
 */
export async function deleteUiCreatedProject(session: AuthedSession, slug: string, projectId: string): Promise<void> {
  const stamp = uniqueSuffixForProjects()
    .replace(/[^a-z0-9]/gi, "")
    .slice(0, 6)
    .toUpperCase();
  const spareId = await createProjectViaApi(session, slug, {
    name: `Spare Default ${stamp}`,
    identifier: `SPD${stamp}`.slice(0, 10),
  });
  const patched = await sessionFetch(session, "PATCH", `/api/workspaces/${slug}/projects/${spareId}/`, {
    is_default: true,
  });
  if (!patched.ok) throw new Error(`[parity] default reassignment failed with HTTP ${patched.status}.`);
  await deleteProjectViaApi(session, slug, projectId);
}

/**
 * The project's published board, or null when nothing is published. The
 * read endpoint returns the single board object (or null/404 when the
 * project was never published or was unpublished again).
 */
export async function serverPublishBoardFacts(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<PublishBoardFacts | null> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`[parity] publish-board read failed with HTTP ${res.status}.`);
  const text = await res.text();
  if (text.trim() === "" || text.trim() === "null") return null;
  const rec = JSON.parse(text) as Record<string, unknown>;
  if (typeof rec["id"] !== "string") return null;
  const bool = (k: string): boolean => rec[k] === true;
  return {
    id: rec["id"] as string,
    anchor: typeof rec["anchor"] === "string" ? (rec["anchor"] as string) : null,
    is_comments_enabled: bool("is_comments_enabled"),
    is_reactions_enabled: bool("is_reactions_enabled"),
    is_votes_enabled: bool("is_votes_enabled"),
    view_props:
      typeof rec["view_props"] === "object" && rec["view_props"] !== null
        ? (rec["view_props"] as Record<string, unknown>)
        : {},
  };
}

// --- Project-settings feature flags oracle (NEWFRONT-393, PSET-043..047).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract.
/** The five feature flags as the project retrieve endpoint reports them. */
export interface ServerProjectFeatureFlags {
  cycleView: boolean;
  moduleView: boolean;
  issueViewsView: boolean;
  pageView: boolean;
  inboxView: boolean;
}

/** Read a project's five feature flags through the retrieve endpoint. */
export async function serverProjectFeatureFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerProjectFeatureFlags> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project flags read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  return {
    cycleView: record["cycle_view"] === true,
    moduleView: record["module_view"] === true,
    issueViewsView: record["issue_views_view"] === true,
    pageView: record["page_view"] === true,
    inboxView: record["inbox_view"] === true,
  };
}

// --- Project-settings automations oracle (NEWFRONT-393, PSET-053/054).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract.
/** The auto-archive/auto-close fields as the project retrieve endpoint reports them. */
export interface ServerProjectAutomationFields {
  archiveIn: unknown;
  closeIn: unknown;
  defaultState: unknown;
}

/** Read a project's auto-archive/auto-close fields through the retrieve endpoint. */
export async function serverProjectAutomationFields(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerProjectAutomationFields> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project automation read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  return {
    archiveIn: record["archive_in"],
    closeIn: record["close_in"],
    defaultState: record["default_state"],
  };
}

// --- Project-settings github tab oracle (NEWFRONT-393, PSET-050/051).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract.
/** The repository binding status as the project repository endpoint reports it. */
export interface ServerRepositoryBinding {
  bound: boolean;
  provider: unknown;
  fullName: unknown;
  webUrl: unknown;
  syncEnabled: boolean;
  lastSyncedAt: unknown;
  lastSyncError: unknown;
}

/** Read a project's repository binding through the repository endpoint. */
export async function serverRepositoryBinding(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerRepositoryBinding> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] repository read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  const repository = (record["repository"] ?? {}) as Record<string, unknown>;
  return {
    bound: record["bound"] === true,
    provider: record["provider"],
    fullName: repository["full_name"],
    webUrl: repository["web_url"],
    syncEnabled: record["is_sync_enabled"] === true,
    lastSyncedAt: record["last_synced_at"],
    lastSyncError: record["last_sync_error"],
  };
}

/**
 * Plant a repository binding (plus one mirrored issue-sync) through the
 * Django shell, mirroring the seeded stack's own plant. The bind endpoint
 * needs provider OAuth the seeded stack cannot mint, so UI unbind coverage
 * plants here instead of binding through the API. Shared seed rows (the
 * provider account and repository) are reused via get_or_create; only the
 * project binding and the sync row are new, and unbinding cascades both.
 * Needs PARITY_API_CONTAINER on namespaced stacks (NEWFRONT-42).
 */
export async function serverPlantRepositoryBinding(
  workspaceSlug: string,
  projectId: string,
  issueId: string
): Promise<string> {
  const python =
    `from pi_dash.db.models import GitIssueSync, GitProviderAccount, GitRepository, GitRepositoryBinding, Issue, Project, Workspace\n` +
    `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
    `proj = Project.objects.get(id=${JSON.stringify(projectId)}, workspace=ws)\n` +
    `account, _ = GitProviderAccount.objects.get_or_create(workspace=ws, provider="github", host_url="https://github.com", defaults={"auth_type": "pat", "created_by_id": proj.created_by_id})\n` +
    `repo, _ = GitRepository.objects.get_or_create(provider="github", host_url="https://github.com", namespace="parity", name="parity-seed", full_name="parity/parity-seed", defaults={"created_by_id": proj.created_by_id})\n` +
    `binding, _ = GitRepositoryBinding.objects.get_or_create(repository=repo, project=proj, defaults={"workspace_id": ws.id, "provider_account": account, "actor_id": proj.created_by_id, "created_by_id": proj.created_by_id})\n` +
    `issue = Issue.objects.get(id=${JSON.stringify(issueId)}, project=proj)\n` +
    `GitIssueSync.objects.get_or_create(binding=binding, issue=issue, defaults={"workspace_id": ws.id, "project_id": proj.id, "provider": "github", "external_iid": "3931", "created_by_id": proj.created_by_id})\n` +
    `print("PARITY_BINDING:" + str(binding.id))\n`;
  const out = await apiShell(python);
  const id = /^PARITY_BINDING:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error("[parity] binding plant produced no id.");
  return id;
}

// --- Workspace-settings members + invitations helpers (NEWFRONT-411,
// --- WSET-008..015). Observed on the running old app: the members list
// --- carries the membership id, nested user record, numeric role
// --- (20/15/5) and the active flag; invitations carry id, email and
// --- numeric role; member role PATCH refuses self changes (400); the
// --- member DELETE endpoint only deactivates (the row persists as
// --- Suspended, and later writes to it 404) while self-removal goes
// --- through the leave endpoint; invitation PATCH/DELETE address the
// --- invitation id. Appended; existing helpers above are untouched per
// --- the shared harness contract.

/** One workspace membership as the server reports it (full identity row). */
export interface ParityWorkspaceMemberFull {
  membershipId: string;
  userId: string;
  displayName: string;
  firstName: string;
  lastName: string;
  email: string;
  role: number;
  isActive: boolean;
  avatarUrl: string | null;
  isBot: boolean;
}

/** Workspace members with roles and activity flags, in API order. */
export async function serverWorkspaceMembersFull(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceMemberFull[]> {
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
      typeof member?.["display_name"] !== "string" ||
      typeof member?.["email"] !== "string" ||
      typeof row["role"] !== "number" ||
      typeof row["is_active"] !== "boolean"
    )
      throw new Error("[parity] workspace member carried no id/user/role/is_active.");
    return {
      membershipId: row["id"] as string,
      userId: member["id"] as string,
      displayName: member["display_name"] as string,
      firstName: typeof member["first_name"] === "string" ? (member["first_name"] as string) : "",
      lastName: typeof member["last_name"] === "string" ? (member["last_name"] as string) : "",
      email: member["email"] as string,
      role: row["role"] as number,
      isActive: row["is_active"] as boolean,
      avatarUrl: typeof member["avatar_url"] === "string" ? (member["avatar_url"] as string) : null,
      isBot: member["is_bot"] === true,
    };
  });
}

/** One pending workspace invitation as the server reports it. */
export interface ParityWorkspaceInvitation {
  id: string;
  email: string;
  role: number;
}

/** Pending workspace invitations, in API order. */
export async function serverWorkspaceInvitations(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceInvitation[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace invitations read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    if (typeof row["id"] !== "string" || typeof row["email"] !== "string" || typeof row["role"] !== "number")
      throw new Error("[parity] invitation row carried no id/email/role.");
    return { id: row["id"] as string, email: row["email"] as string, role: row["role"] as number };
  });
}

/**
 * Invite email-plus-role rows in one bulk call (the dialog's own shape),
 * then resolve the created invitation rows. The create response carries
 * only a message, so the rows come from a follow-up list read.
 */
export async function serverCreateWorkspaceInvitations(
  workspaceSlug: string,
  invites: { email: string; role: number }[],
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceInvitation[]> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/invitations/`, sessionCookie, {
    emails: invites.map((invite) => ({ email: invite.email, role: invite.role })),
  });
  if (!res.ok) throw new Error(`[parity] invitation bulk create failed with HTTP ${res.status}.`);
  const rows = await serverWorkspaceInvitations(workspaceSlug, sessionCookie, apiBase);
  const wanted = new Set(invites.map((invite) => invite.email.toLowerCase()));
  const created = rows.filter((row) => wanted.has(row.email.toLowerCase()));
  if (created.length !== invites.length)
    throw new Error("[parity] invitation bulk create left some invitees unlisted.");
  return created;
}

/** Strict invitation role PATCH (setup and state assertions; throws on failure). */
export async function serverPatchWorkspaceInvitation(
  workspaceSlug: string,
  invitationId: string,
  data: { role: number },
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceInvitation> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/invitations/${invitationId}/`,
    sessionCookie,
    data
  );
  if (!res.ok) throw new Error(`[parity] invitation patch failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  if (typeof row["id"] !== "string" || typeof row["email"] !== "string" || typeof row["role"] !== "number")
    throw new Error("[parity] invitation patch response carried no id/email/role.");
  return { id: row["id"] as string, email: row["email"] as string, role: row["role"] as number };
}

/** Lenient invitation removal for finally-blocks. */
export async function serverCleanupWorkspaceInvitation(
  workspaceSlug: string,
  invitationId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/invitations/${invitationId}/`,
    sessionCookie
  );
  if (!res.ok) console.log(`[parity] invitation cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}

/** Strict member role PATCH (setup; throws on failure). */
export async function serverPatchWorkspaceMemberRole(
  workspaceSlug: string,
  membershipId: string,
  role: number,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/members/${membershipId}/`,
    sessionCookie,
    { role }
  );
  if (!res.ok) throw new Error(`[parity] workspace member patch failed with HTTP ${res.status}.`);
}

/**
 * Hard-delete a workspace membership row through the Django shell. The
 * member DELETE endpoint only deactivates (the row persists as
 * Suspended), so specs that need the row gone — teardown, or a
 * failure setup whose UI write must 404 — delete here instead.
 * Idempotent: deleting a missing row is a no-op. Needs
 * PARITY_API_CONTAINER on namespaced stacks (NEWFRONT-42).
 */
export async function serverHardDeleteWorkspaceMember(membershipId: string): Promise<void> {
  // SoftDeleteModel overrides delete(): plain .delete() only stamps
  // deleted_at and returns an int; soft=False performs the real delete
  // and returns the usual (count, labels) tuple.
  const out = await apiShell(
    `from pi_dash.db.models.workspace import WorkspaceMember\n` +
      `deleted, _ = WorkspaceMember.objects.filter(id=${JSON.stringify(membershipId)}).delete(soft=False)\n` +
      `print("PARITY_DELETED:" + str(deleted))\n`
  );
  if (!/^PARITY_DELETED:\d+$/m.test(out)) throw new Error("[parity] membership hard delete produced no count.");
}

// --- Project members tab (NEWFRONT-391, PSET-018–027). Appended; existing
// --- helpers above are untouched per the shared harness contract.

/** Project admin role id (siblings PROJECT_ROLE_GUEST/MEMBER live above). */
export const PROJECT_ROLE_ADMIN = 20;

/** One project membership as the members endpoint reports it. */
export interface ParityProjectMember {
  /** Membership row id (addresses the PATCH/DELETE detail endpoints). */
  membershipId: string;
  /** User id of the member. */
  memberId: string;
  displayName: string;
  firstName: string;
  lastName: string;
  email: string;
  /** Project role number (20 admin, 15 member, 5 guest). */
  role: number;
  /** Joining timestamp as the server sent it (may be ""). */
  joinedAt: string;
}

/**
 * One membership's detail as the member endpoint reports it: the list rows
 * carry only UUID references, so details are read per membership id and
 * joined with the list row's project role.
 */
async function fetchProjectMemberIdentity(
  workspaceSlug: string,
  projectId: string,
  membershipId: string,
  sessionCookie: string,
  apiBase: string
): Promise<{ memberId: string; displayName: string; firstName: string; lastName: string; email: string }> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/${membershipId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project member read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as { member?: unknown };
  const member =
    record.member !== undefined && typeof record.member === "object" && record.member !== null
      ? (record.member as Record<string, unknown>)
      : null;
  if (
    member === null ||
    typeof member["id"] !== "string" ||
    typeof member["display_name"] !== "string" ||
    typeof member["email"] !== "string"
  ) {
    throw new Error("[parity] project member detail carried no usable member object.");
  }
  return {
    memberId: member["id"] as string,
    displayName: member["display_name"] as string,
    firstName: typeof member["first_name"] === "string" ? member["first_name"] : "",
    lastName: typeof member["last_name"] === "string" ? member["last_name"] : "",
    email: member["email"] as string,
  };
}

/**
 * Project memberships with user identity in API order; throws unless the
 * server answers 2xx. (Sibling serverProjectMembers carries only user ids
 * and roles; this variant joins the per-membership detail for names and
 * emails, which the members-table scenarios assert.)
 */
export async function serverProjectMemberProfiles(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectMember[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project members read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const members: ParityProjectMember[] = [];
  for (const row of rows) {
    const record = row as { id?: unknown; role?: unknown; original_role?: unknown; created_at?: unknown };
    if (typeof record.id !== "string") throw new Error("[parity] project member row carried no string id.");
    const role = record.original_role ?? record.role;
    if (typeof role !== "number") throw new Error("[parity] project member row carried no numeric role.");
    const detail = await fetchProjectMemberIdentity(workspaceSlug, projectId, record.id, sessionCookie, apiBase);
    members.push({
      membershipId: record.id,
      ...detail,
      role,
      joinedAt: typeof record.created_at === "string" ? record.created_at : "",
    });
  }
  return members;
}

/** One membership by member email, or null when not on the project. */
export async function serverProjectMemberByEmail(
  workspaceSlug: string,
  projectId: string,
  email: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectMember | null> {
  const members = await serverProjectMemberProfiles(workspaceSlug, projectId, sessionCookie, apiBase);
  return members.find((m) => m.email.toLowerCase() === email.toLowerCase()) ?? null;
}

/** Change one member's project role by membership id; throws unless the server accepts. */
export async function serverPatchProjectMember(
  workspaceSlug: string,
  projectId: string,
  memberId: string,
  sessionCookie: string,
  role: number,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/${memberId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ role }),
  });
  if (!res.ok) throw new Error(`[parity] project member patch failed with HTTP ${res.status}.`);
}

/** Remove one member from a project by membership id; throws unless the server accepts. */
export async function serverRemoveProjectMember(
  workspaceSlug: string,
  projectId: string,
  memberId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/${memberId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] project member remove failed with HTTP ${res.status}.`);
}

/** Leave a project as the session user; throws unless the server accepts. */
export async function serverLeaveProject(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/members/leave/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
  });
  if (res.status !== 200 && res.status !== 204)
    throw new Error(`[parity] project leave failed with HTTP ${res.status}.`);
}

/** Lead / default-assignee / guest-visibility fields of one project. */
export interface ParityProjectSettings {
  projectLeadId: string | null;
  defaultAssigneeId: string | null;
  guestViewAll: boolean;
}

/** Read one project's lead, default assignee and guest-visibility flag. */
export async function serverProjectSettings(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityProjectSettings> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project settings read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as {
    project_lead?: unknown;
    default_assignee?: unknown;
    guest_view_all_features?: unknown;
  };
  const idOrNull = (value: unknown): string | null => {
    if (value === null || value === undefined) return null;
    if (typeof value === "string") return value === "" ? null : value;
    if (typeof value === "object") {
      const id = (value as { id?: unknown }).id;
      return typeof id === "string" ? id : null;
    }
    return null;
  };
  return {
    projectLeadId: idOrNull(record.project_lead),
    defaultAssigneeId: idOrNull(record.default_assignee),
    guestViewAll: record.guest_view_all_features === true,
  };
}

/**
 * Change one workspace member's role by workspace-membership id; throws
 * unless the server accepts. Member<->admin moves leave project roles
 * untouched (only a move to guest cascades), so promotion/restore pairs
 * are stack-safe for staging a workspace-admin viewer.
 */
export async function serverPatchWorkspaceMember(
  workspaceSlug: string,
  membershipId: string,
  sessionCookie: string,
  role: number,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/members/${membershipId}/`, {
    method: "PATCH",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ role }),
  });
  if (!res.ok) throw new Error(`[parity] workspace member patch failed with HTTP ${res.status}.`);
}

/** End of the NEWFRONT-391 project-members-tab block. */

// --- Workspace-settings cross-cutting oracle (NEWFRONT-415, WSET-033..040).
// --- Webhook CRUD + status reads, workspace-role reads, and API-token list
// --- reads for the exports-entry, deep-link, and negative scenarios. Every
// --- helper goes through the public REST API with a caller-owned session;
// --- nothing here touches the DOM. Appended; existing helpers above are
// --- untouched per the shared parity contract.

/** A workspace webhook row as the server reports it. */
export interface ParityWorkspaceWebhook {
  id: string;
  url: string;
  isActive: boolean;
}

function parityWebhookOf(record: Record<string, unknown>): ParityWorkspaceWebhook {
  if (typeof record["id"] !== "string" || typeof record["url"] !== "string") {
    throw new Error("[parity] webhook row carried no id/url.");
  }
  return { id: record["id"], url: record["url"], isActive: record["is_active"] === true };
}

/** Create a webhook; resolves with the created row (secret is write-only). */
export async function serverCreateWorkspaceWebhook(
  admin: AuthedSession,
  slug: string,
  input: { url: string }
): Promise<ParityWorkspaceWebhook> {
  const res = await sessionFetch(admin, "POST", `/api/workspaces/${slug}/webhooks/`, {
    url: input.url,
    is_active: true,
  });
  if (res.status !== 200 && res.status !== 201) {
    throw new Error(`[parity] webhook create failed with HTTP ${res.status}.`);
  }
  return parityWebhookOf((await res.json()) as Record<string, unknown>);
}

/** Delete a webhook (best effort: 404 means already gone). */
export async function serverDeleteWorkspaceWebhook(
  admin: AuthedSession,
  slug: string,
  webhookId: string
): Promise<void> {
  const res = await sessionFetch(admin, "DELETE", `/api/workspaces/${slug}/webhooks/${webhookId}/`);
  if (!res.ok && res.status !== 204 && res.status !== 404) {
    throw new Error(`[parity] webhook delete failed with HTTP ${res.status}.`);
  }
}

/** Raw detail status for one webhook id (proves unknown ids 404 server-side). */
export async function serverWorkspaceWebhookStatus(
  admin: AuthedSession,
  slug: string,
  webhookId: string
): Promise<number> {
  const res = await sessionFetch(admin, "GET", `/api/workspaces/${slug}/webhooks/${webhookId}/`);
  return res.status;
}

/** Every webhook on a workspace, in server order. */
export async function serverListWorkspaceWebhooks(
  admin: AuthedSession,
  slug: string
): Promise<ParityWorkspaceWebhook[]> {
  const res = await sessionFetch(admin, "GET", `/api/workspaces/${slug}/webhooks/`);
  if (!res.ok) throw new Error(`[parity] webhook list read failed with HTTP ${res.status}.`);
  return ((await res.json()) as Record<string, unknown>[]).map(parityWebhookOf);
}

/** Numeric workspace role of a user (20/15/5), or null when not a member. */
export async function serverWorkspaceMemberRole(user: AuthedSession, slug: string): Promise<number | null> {
  const res = await sessionFetch(user, "GET", `/api/workspaces/${slug}/members/`);
  if (!res.ok) throw new Error(`[parity] workspace members read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  for (const row of rows) {
    const member = row["member"] as Record<string, unknown> | undefined;
    if (member?.["id"] === user.userId) {
      return typeof row["role"] === "number" ? (row["role"] as number) : null;
    }
  }
  return null;
}

/** Personal access tokens of the caller's account (id + label only). */
export async function serverListApiTokens(user: AuthedSession): Promise<Array<{ id: string; label: string }>> {
  const res = await sessionFetch(user, "GET", `/api/users/api-tokens/`);
  if (!res.ok) throw new Error(`[parity] api-token list read failed with HTTP ${res.status}.`);
  return ((await res.json()) as Array<Record<string, unknown>>).map((row) => {
    if (typeof row["id"] !== "string") throw new Error("[parity] api-token row carried no id.");
    return { id: row["id"], label: typeof row["label"] === "string" ? row["label"] : "" };
  });
}

// --- Workspace-settings shell + general helpers (NEWFRONT-410,
// --- WSET-001..007). Appended; existing helpers above are untouched per
// --- the shared harness contract. Typed reads of the workspace-detail
// --- endpoint back the general-page and delete-zone scenarios; the
// --- generic serverRequestStatus covers refusal-code assertions.

/** Workspace-detail facts the general-page scenarios assert on. */
export interface ParityWorkspaceFacts {
  id: string;
  slug: string;
  name: string;
  organizationSize: string | null;
  timezone: string;
  logoUrl: string | null;
}

/** Typed read of GET /api/workspaces/{slug}/ (throws unless HTTP 200). */
export async function serverWorkspace(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityWorkspaceFacts> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  if (typeof row["id"] !== "string" || typeof row["slug"] !== "string" || typeof row["name"] !== "string") {
    throw new Error("[parity] workspace row carried no id, slug and name.");
  }
  const size = row["organization_size"];
  const zone = row["timezone"];
  const logo = row["logo_url"];
  return {
    id: row["id"] as string,
    slug: row["slug"] as string,
    name: row["name"] as string,
    organizationSize: typeof size === "string" ? size : null,
    timezone: typeof zone === "string" ? zone : "",
    logoUrl: typeof logo === "string" && logo !== "" ? logo : null,
  };
}

/** Lenient workspace deletion for finally-blocks (the UI delete is the real path). */
export async function serverDeleteWorkspace(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON("DELETE", `${apiBase}/api/workspaces/${workspaceSlug}/`, sessionCookie);
  if (!res.ok) console.log(`[parity] workspace cleanup DELETE returned HTTP ${res.status}; leaving it.`);
}

// --- Workspace-settings developer oracle (NEWFRONT-412, WSET-016..024).
// --- Webhook CRUD + integration reads live here; provider connects verify
// --- tokens against the live providers, unreachable from the seeded stack,
// --- so connected states are planted through the Django shell (mirroring
// --- what the connect endpoints store) and removed the same way. Appended;
// --- existing helpers above are untouched per the shared contract.

/** One workspace webhook row as the list/detail endpoints return it. */
export interface ServerWebhook {
  id: string;
  url: string;
  is_active: boolean;
  project: boolean;
  issue: boolean;
  cycle: boolean;
  module: boolean;
  issue_comment: boolean;
}

function toServerWebhook(record: Record<string, unknown>): ServerWebhook {
  const text = (key: string): string => (typeof record[key] === "string" ? (record[key] as string) : "");
  const flag = (key: string): boolean => record[key] === true;
  return {
    id: text("id"),
    url: text("url"),
    is_active: flag("is_active"),
    project: flag("project"),
    issue: flag("issue"),
    cycle: flag("cycle"),
    module: flag("module"),
    issue_comment: flag("issue_comment"),
  };
}

/** List a workspace's webhooks (admin session). */
export async function serverWebhooksList(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerWebhook[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/webhooks/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] webhooks list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map(toServerWebhook);
}

/** Read one webhook by id (admin session). */
export async function serverWebhookDetails(
  workspaceSlug: string,
  webhookId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerWebhook> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/webhooks/${webhookId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] webhook read failed with HTTP ${res.status}.`);
  return toServerWebhook((await res.json()) as Record<string, unknown>);
}

/** Create a webhook; the response carries the one-time secret key. */
export async function serverCreateWebhook(
  workspaceSlug: string,
  data: { url: string; project?: boolean; issue?: boolean; cycle?: boolean; module?: boolean; issue_comment?: boolean },
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerWebhook & { secret_key: string }> {
  const res = await mutateJSON("POST", `${apiBase}/api/workspaces/${workspaceSlug}/webhooks/`, sessionCookie, data);
  if (!res.ok) throw new Error(`[parity] webhook create failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  return {
    ...toServerWebhook(record),
    secret_key: typeof record["secret_key"] === "string" ? (record["secret_key"] as string) : "",
  };
}

/** Best-effort webhook cleanup; logs instead of throwing. */
export async function serverCleanupWebhook(
  workspaceSlug: string,
  webhookId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "DELETE",
    `${apiBase}/api/workspaces/${workspaceSlug}/webhooks/${webhookId}/`,
    sessionCookie
  );
  if (!res.ok && res.status !== 404)
    console.log(`[parity] webhook cleanup DELETE returned HTTP ${res.status}; leaving it for reseed.`);
}

/** GitHub workspace-connection status as the status endpoint returns it. */
export interface ServerGithubStatus {
  connected: boolean;
  github_user_login: string;
}

/** Read the GitHub connection status (admin or member session). */
export async function serverGithubStatus(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerGithubStatus> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/integrations/github/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] github status read failed with HTTP ${res.status}.`);
  const record = (await res.json()) as Record<string, unknown>;
  return {
    connected: record["connected"] === true,
    github_user_login: typeof record["github_user_login"] === "string" ? (record["github_user_login"] as string) : "",
  };
}

/** One git provider account as the accounts endpoint returns it. */
export interface ServerGitAccount {
  id: string;
  provider: string;
  host_url: string;
  external_account_login: string;
  display_name: string;
  status: string;
}

/** List a workspace's git provider accounts. */
export async function serverGitAccounts(
  workspaceSlug: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerGitAccount[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/integrations/git/accounts/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] git accounts list failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { accounts?: Record<string, unknown>[] };
  return (payload.accounts ?? []).map((record) => ({
    id: typeof record["id"] === "string" ? (record["id"] as string) : "",
    provider: typeof record["provider"] === "string" ? (record["provider"] as string) : "",
    host_url: typeof record["host_url"] === "string" ? (record["host_url"] as string) : "",
    external_account_login:
      typeof record["external_account_login"] === "string" ? (record["external_account_login"] as string) : "",
    display_name: typeof record["display_name"] === "string" ? (record["display_name"] as string) : "",
    status: typeof record["status"] === "string" ? (record["status"] as string) : "",
  }));
}

/**
 * Plant a connected GitHub workspace integration through the Django shell,
 * mirroring what the connect endpoint stores (provider row, FK-shim token,
 * encrypted credential plus login in config). The connect endpoint verifies
 * the PAT against live GitHub, unreachable from the seeded stack, so the UI
 * connect/disconnect coverage plants here instead of connecting through the
 * API. Needs PARITY_API_CONTAINER on namespaced stacks.
 */
export async function serverSeedGithubConnection(workspaceSlug: string, login: string): Promise<void> {
  const python =
    `from django.utils import timezone\n` +
    `from pi_dash.db.models import APIToken, Integration, Workspace, WorkspaceIntegration, WorkspaceMember\n` +
    `from pi_dash.license.utils.encryption import encrypt_data\n` +
    `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
    `actor = WorkspaceMember.objects.filter(workspace=ws, role=20).first().member\n` +
    `integration, _ = Integration.objects.get_or_create(provider="github", defaults={"title": "GitHub", "verified": True, "description": {"summary": "Mirror GitHub issues into Pi Dash projects."}})\n` +
    `for ghost in WorkspaceIntegration.all_objects.filter(workspace=ws, integration=integration, deleted_at__isnull=False):\n` +
    `    ghost.delete(soft=False)\n` +
    `wi = WorkspaceIntegration.objects.filter(workspace=ws, integration=integration).first()\n` +
    `if wi is None:\n` +
    `    shim = APIToken.objects.create(user=actor, workspace=ws, user_type=1, is_active=False, label="github-integration-" + str(ws.id), description="GitHub integration FK shim — not for auth")\n` +
    `    wi = WorkspaceIntegration.objects.create(workspace=ws, actor=actor, integration=integration, api_token=shim, config={})\n` +
    `wi.config = {"auth_type": "pat", "token": encrypt_data("parity-planted-github-token"), "github_user_login": ${JSON.stringify(login)}, "verified_at": timezone.now().isoformat()}\n` +
    `wi.save(update_fields=["config"])\n` +
    `print("PARITY_GITHUB_SEEDED")\n`;
  const out = await apiShell(python);
  if (!out.includes("PARITY_GITHUB_SEEDED")) throw new Error("[parity] github plant produced no marker.");
}

/**
 * Remove the planted GitHub workspace integration (plus its FK-shim token)
 * through the Django shell. Best-effort: logs instead of throwing so
 * finally-blocks stay safe.
 */
export async function serverUnseedGithubConnection(workspaceSlug: string): Promise<void> {
  try {
    // Hard deletes throughout: the workspace+integration unique counts
    // soft-deleted rows, so a soft unseed would brick the next plant.
    const python =
      `from pi_dash.db.models import APIToken, Integration, Workspace, WorkspaceIntegration\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `integration = Integration.objects.filter(provider="github").first()\n` +
      `for wi in WorkspaceIntegration.all_objects.filter(workspace=ws, integration=integration):\n` +
      `    token_id = wi.api_token_id\n` +
      `    wi.delete(soft=False)\n` +
      `    APIToken.all_objects.filter(id=token_id).delete()\n` +
      `used = WorkspaceIntegration.all_objects.filter(workspace=ws).values_list("api_token_id", flat=True)\n` +
      `APIToken.all_objects.filter(workspace=ws, label="github-integration-" + str(ws.id), is_active=False).exclude(id__in=used).delete()\n` +
      `print("PARITY_GITHUB_UNSEEDED")\n`;
    const out = await apiShell(python);
    if (!out.includes("PARITY_GITHUB_UNSEEDED")) throw new Error("[parity] github unseed produced no marker.");
  } catch (error) {
    console.log(`[parity] github unseed skipped: ${error instanceof Error ? error.message : error}`);
  }
}

/**
 * Plant a sync-enabled repository binding (plus its provider account)
 * through the Django shell so UI disconnects have a dependent sync to
 * disable. For provider github with linkWorkspaceIntegration, the account
 * attaches to the planted workspace integration (plant it first via
 * serverSeedGithubConnection); otherwise the account stands alone. The
 * shared seed repository row is reused for github; gitlab gets its own.
 * Returns the account and binding ids. Needs PARITY_API_CONTAINER on
 * namespaced stacks.
 */
export async function serverPlantGitSyncBinding(
  workspaceSlug: string,
  projectId: string,
  opts: { provider: "github" | "gitlab"; login: string; hostUrl: string; linkWorkspaceIntegration: boolean }
): Promise<{ accountId: string; bindingId: string }> {
  const repoFullName = opts.provider === "github" ? "parity/parity-seed" : "parity/parity-412-gitlab";
  const python =
    `from pi_dash.db.models import GitProviderAccount, GitRepository, GitRepositoryBinding, Integration, Project, Workspace, WorkspaceIntegration, WorkspaceMember\n` +
    `from pi_dash.license.utils.encryption import encrypt_data\n` +
    `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
    `proj = Project.objects.get(id=${JSON.stringify(projectId)}, workspace=ws)\n` +
    `actor_id = WorkspaceMember.objects.filter(workspace=ws, role=20).first().member_id\n` +
    `wi_id = None\n` +
    (opts.linkWorkspaceIntegration
      ? `integration = Integration.objects.filter(provider="github").first()\n` +
        `wi = WorkspaceIntegration.objects.filter(workspace=ws, integration=integration).first() if integration else None\n` +
        `wi_id = wi.id if wi else None\n`
      : ``) +
    `account = GitProviderAccount.objects.create(workspace=ws, provider=${JSON.stringify(opts.provider)}, host_url=${JSON.stringify(opts.hostUrl)}, auth_type="pat", external_account_id=${JSON.stringify(`parity-${opts.login}`)}, external_account_login=${JSON.stringify(opts.login)}, display_name=${JSON.stringify(opts.login)}, capabilities={}, credential_config={"auth_type": "pat", "host_url": ${JSON.stringify(opts.hostUrl)}, "token": encrypt_data("parity-planted-cascade-token")}, workspace_integration_id=wi_id, status="connected", metadata={}, created_by_id=actor_id, updated_by_id=actor_id)\n` +
    `repo, _ = GitRepository.objects.get_or_create(provider=${JSON.stringify(opts.provider)}, host_url=${JSON.stringify(opts.hostUrl)}, full_name=${JSON.stringify(repoFullName)}, defaults={"namespace": "parity", "name": ${JSON.stringify(repoFullName.split("/")[1] ?? repoFullName)}, "created_by_id": actor_id})\n` +
    `binding = GitRepositoryBinding.objects.create(workspace_id=ws.id, project=proj, repository=repo, provider_account=account, actor_id=actor_id, is_sync_enabled=True, created_by_id=actor_id, updated_by_id=actor_id)\n` +
    `print("PARITY_CASCADE:" + str(account.id) + ":" + str(binding.id))\n`;
  const out = await apiShell(python);
  const match = /^PARITY_CASCADE:(.+):(.+)$/m.exec(out);
  const accountId = match?.[1]?.trim() ?? "";
  const bindingId = match?.[2]?.trim() ?? "";
  if (accountId === "" || bindingId === "") throw new Error("[parity] cascade plant produced no ids.");
  return { accountId, bindingId };
}

/**
 * Hard-delete a planted GitLab provider account through the Django shell
 * (the API delete only revokes). Best-effort: logs instead of throwing.
 */
export async function serverUnseedGitAccount(accountId: string): Promise<void> {
  try {
    // Hard delete (plain manager): soft-deleted accounts linger otherwise.
    // Bindings go first: even a soft-deleted binding still PROTECTs its
    // account through the base manager.
    const python =
      `from pi_dash.db.models import GitProviderAccount, GitRepositoryBinding\n` +
      `GitRepositoryBinding.all_objects.filter(provider_account_id=${JSON.stringify(accountId)}).delete()\n` +
      `GitProviderAccount.all_objects.filter(id=${JSON.stringify(accountId)}).delete()\n` +
      `print("PARITY_GITLAB_UNSEEDED")\n`;
    const out = await apiShell(python);
    if (!out.includes("PARITY_GITLAB_UNSEEDED")) throw new Error("[parity] gitlab unseed produced no marker.");
  } catch (error) {
    console.log(`[parity] gitlab unseed skipped: ${error instanceof Error ? error.message : error}`);
  }
}

// --- Profile-settings upkeep + GitHub App server reads (NEWFRONT-428,
// --- PROF-024, PROF-025). Typed reads/writes for the auto-pm master and
// --- per-job toggles and the personal GitHub App connection, plus
// --- Django-shell fixtures for instance-level state the UI cannot create
// --- (scheduled upkeep jobs, a connected installation). Assistant, STT,
// --- and tool-server CRUD plus connection-test probes already live above
// --- (serverAssistant*); this block adds nothing parallel. Appended;
// --- existing helpers above are untouched per the shared driver contract.

/** One scheduled upkeep job as the auto-pm endpoint returns it. */
export interface ServerAutoPmJob {
  slug: string;
  name: string;
  description: string;
  intervalLabel: string;
  enabled: boolean;
}

/** The caller's upkeep settings: master switch plus scheduled jobs. */
export interface ServerAutoPmSettings {
  enabled: boolean;
  jobs: ServerAutoPmJob[];
}

function autoPmSettingsOf(payload: unknown): ServerAutoPmSettings {
  const row = payload as Record<string, unknown>;
  const jobs = row["jobs"];
  if (!Array.isArray(jobs)) throw new Error("[parity] upkeep settings carried no jobs array.");
  return {
    enabled: row["enabled"] === true,
    jobs: jobs.map((entry) => {
      const job = entry as Record<string, unknown>;
      if (typeof job["slug"] !== "string" || typeof job["name"] !== "string") {
        throw new Error("[parity] upkeep job carried no string slug/name.");
      }
      return {
        slug: job["slug"],
        name: job["name"],
        description: typeof job["description"] === "string" ? job["description"] : "",
        intervalLabel: typeof job["interval_label"] === "string" ? job["interval_label"] : "",
        enabled: job["enabled"] === true,
      };
    }),
  };
}

/** Read the caller's upkeep settings (master switch + scheduled jobs). */
export async function serverAutoPmGet(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAutoPmSettings> {
  const res = await apiJson("GET", "/api/users/me/auto-pm/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] upkeep read failed with HTTP ${res.status}.`);
  return autoPmSettingsOf(res.payload);
}

/** Set the master pause switch (true resumes, false pauses everything). */
export async function serverAutoPmSetMaster(
  sessionCookie: string,
  enabled: boolean,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAutoPmSettings> {
  const res = await mutateJSON("PATCH", `${apiBase}/api/users/me/auto-pm/`, sessionCookie, { enabled });
  if (res.status !== 200) throw new Error(`[parity] upkeep master write failed with HTTP ${res.status}.`);
  return autoPmSettingsOf((await res.json()) as unknown);
}

/** Set one scheduled job's switch. */
export async function serverAutoPmSetJob(
  sessionCookie: string,
  slug: string,
  enabled: boolean,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAutoPmSettings> {
  const res = await serverAutoPmSetJobRaw(sessionCookie, slug, enabled, apiBase);
  if (res.status !== 200) throw new Error(`[parity] upkeep job write failed with HTTP ${res.status}.`);
  return autoPmSettingsOf(res.payload);
}

/** Raw job write for the unknown-slug refusal half (404 not_found). */
export async function serverAutoPmSetJobRaw(
  sessionCookie: string,
  slug: string,
  enabled: boolean,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson("PATCH", `/api/users/me/auto-pm/jobs/${slug}/`, sessionCookie, { enabled }, apiBase);
}

/**
 * Flip a loop job's instance-level enabled flag through the Django shell.
 * The seeded stack schedules nothing (the builtin seeds enabled=False for
 * an operator smoke test), so toggle scenarios enable here and restore
 * afterwards; reruns round-trip the same flag. Needs PARITY_API_CONTAINER
 * on namespaced stacks.
 */
export async function serverLoopJobSetEnabled(slug: string, enabled: boolean): Promise<void> {
  const python =
    `from pi_dash.db.models import LoopJob\n` +
    `n = LoopJob.objects.filter(slug=${JSON.stringify(slug)}).update(enabled=${enabled ? "True" : "False"})\n` +
    `print(f"PARITY_LOOP_JOB:{n}")\n`;
  const out = await apiShell(python);
  if (!out.includes("PARITY_LOOP_JOB:1")) throw new Error("[parity] loop-job flag update touched no row.");
}

/** One workspace's GitHub App connection facts. */
export interface ServerGithubAppInstallation {
  connected: boolean;
  installationId: number | null;
  accountLogin: string;
  accountType: string;
  repositorySelection: string;
  repositoryCount: number;
  verifiedAt: string | null;
  lastCheckedAt: string | null;
  lastCheckError: string;
  suspended: boolean;
}

/** One admin workspace's GitHub App status row. */
export interface ServerGithubAppWorkspace {
  id: string;
  slug: string;
  name: string;
  githubApp: ServerGithubAppInstallation;
}

/** The caller's GitHub App status across admin workspaces. */
export interface ServerGithubAppStatus {
  configured: boolean;
  appSlug: string;
  workspaces: ServerGithubAppWorkspace[];
}

function githubAppInstallationOf(row: unknown): ServerGithubAppInstallation {
  const record = row as Record<string, unknown>;
  return {
    connected: record["connected"] === true,
    installationId: typeof record["installation_id"] === "number" ? record["installation_id"] : null,
    accountLogin: typeof record["account_login"] === "string" ? record["account_login"] : "",
    accountType: typeof record["account_type"] === "string" ? record["account_type"] : "",
    repositorySelection: typeof record["repository_selection"] === "string" ? record["repository_selection"] : "",
    repositoryCount: typeof record["repository_count"] === "number" ? record["repository_count"] : 0,
    verifiedAt: typeof record["verified_at"] === "string" ? record["verified_at"] : null,
    lastCheckedAt: typeof record["last_checked_at"] === "string" ? record["last_checked_at"] : null,
    lastCheckError: typeof record["last_check_error"] === "string" ? record["last_check_error"] : "",
    suspended: typeof record["suspended_at"] === "string" && record["suspended_at"] !== "",
  };
}

/** Read the caller's GitHub App status across admin workspaces. */
export async function serverGithubAppStatus(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerGithubAppStatus> {
  const res = await apiJson("GET", "/api/users/me/integrations/github/app/", sessionCookie, undefined, apiBase);
  if (res.status !== 200) throw new Error(`[parity] github-app status read failed with HTTP ${res.status}.`);
  const payload = res.payload as Record<string, unknown>;
  const workspaces = payload["workspaces"];
  if (!Array.isArray(workspaces)) throw new Error("[parity] github-app status carried no workspaces array.");
  return {
    configured: payload["configured"] === true,
    appSlug: typeof payload["app_slug"] === "string" ? payload["app_slug"] : "",
    workspaces: workspaces.map((entry) => {
      const row = entry as Record<string, unknown>;
      if (typeof row["slug"] !== "string" || typeof row["name"] !== "string") {
        throw new Error("[parity] github-app workspace carried no string slug/name.");
      }
      return {
        id: typeof row["id"] === "string" ? row["id"] : "",
        slug: row["slug"],
        name: row["name"],
        githubApp: githubAppInstallationOf(row["github_app"]),
      };
    }),
  };
}

/** Raw install-start outcome (409 while the instance config is missing). */
export async function serverGithubAppInstallStartRaw(
  sessionCookie: string,
  workspaceSlug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson(
    "POST",
    "/api/users/me/integrations/github/app/install/",
    sessionCookie,
    { workspace_slug: workspaceSlug },
    apiBase
  );
}

/** Raw refresh outcome (404 uninstalled, 502 when GitHub cannot be reached). */
export async function serverGithubAppRefreshRaw(
  sessionCookie: string,
  workspaceSlug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; payload: unknown }> {
  return apiJson(
    "POST",
    "/api/users/me/integrations/github/app/refresh/",
    sessionCookie,
    { workspace_slug: workspaceSlug },
    apiBase
  );
}

/**
 * Plant a connected GitHub App installation for a workspace through the
 * Django shell, mirroring what the install callback stores (provider row,
 * FK-shim token, workspace link, installation facts). The real install
 * needs GitHub OAuth, unreachable from the seeded stack, so the
 * connected-panel and manual-refresh coverage plants here instead of
 * installing through the API. Needs PARITY_API_CONTAINER on namespaced
 * stacks.
 */
export async function serverSeedGithubAppInstallation(workspaceSlug: string, login: string): Promise<void> {
  const python =
    `from django.utils import timezone\n` +
    `from pi_dash.db.models import APIToken, GithubAppInstallation, Integration, Workspace, WorkspaceIntegration, WorkspaceMember\n` +
    `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
    `actor = WorkspaceMember.objects.filter(workspace=ws, role=20).first().member\n` +
    `integration, _ = Integration.objects.get_or_create(provider="github", defaults={"title": "GitHub", "verified": True, "description": {"summary": "Mirror GitHub issues into Pi Dash projects."}})\n` +
    `for ghost in WorkspaceIntegration.all_objects.filter(workspace=ws, integration=integration, deleted_at__isnull=False):\n` +
    `    ghost.delete(soft=False)\n` +
    `wi = WorkspaceIntegration.objects.filter(workspace=ws, integration=integration).first()\n` +
    `if wi is None:\n` +
    `    shim = APIToken.objects.create(user=actor, workspace=ws, user_type=1, is_active=False, label="github-app-plant-" + str(ws.id), description="GitHub App plant FK shim — not for auth")\n` +
    `    wi = WorkspaceIntegration.objects.create(workspace=ws, actor=actor, integration=integration, api_token=shim, config={})\n` +
    `GithubAppInstallation.all_objects.filter(workspace_integration=wi).delete()\n` +
    `now = timezone.now()\n` +
    `GithubAppInstallation.objects.create(workspace_integration=wi, installation_id=42428001, account_login=${JSON.stringify(login)}, account_type="Organization", repository_selection="selected", repository_count=3, permissions={"issues": "write"}, events=["issues"], installed_at=now, verified_at=now, last_checked_at=now, last_check_error="")\n` +
    `print("PARITY_GITHUB_APP_SEEDED")\n`;
  const out = await apiShell(python);
  if (!out.includes("PARITY_GITHUB_APP_SEEDED")) throw new Error("[parity] github-app plant produced no marker.");
}

/**
 * Remove the planted GitHub App installation (plus its workspace link and
 * FK-shim token) through the Django shell. Best-effort: logs instead of
 * throwing so finally-blocks stay safe.
 */
export async function serverUnseedGithubAppInstallation(workspaceSlug: string): Promise<void> {
  try {
    const python =
      `from pi_dash.db.models import APIToken, GithubAppInstallation, Integration, Workspace, WorkspaceIntegration\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(workspaceSlug)})\n` +
      `integration = Integration.objects.filter(provider="github").first()\n` +
      `for wi in WorkspaceIntegration.all_objects.filter(workspace=ws, integration=integration):\n` +
      `    token_id = wi.api_token_id\n` +
      `    GithubAppInstallation.all_objects.filter(workspace_integration_id=wi.id).delete()\n` +
      `    wi.delete(soft=False)\n` +
      `    APIToken.all_objects.filter(id=token_id).delete()\n` +
      `print("PARITY_GITHUB_APP_UNSEEDED")\n`;
    const out = await apiShell(python);
    if (!out.includes("PARITY_GITHUB_APP_UNSEEDED")) {
      console.log("[parity] github-app unseed produced no marker; leaving it for reseed.");
    }
  } catch (error) {
    console.log(`[parity] github-app unseed failed (${String(error)}); leaving it for reseed.`);
  }
}

// ---------------------------------------------------------------------------
// Profile-settings account assertions (NEWFRONT-426, rows PROF-011..015). The
// signed-in user's record (names, email, avatar, password mode), the instance
// gates behind the email-change and deactivation controls, and minted email-
// update codes standing in for the mailed code. Appended; existing entries
// above are untouched per the shared contract.
// ---------------------------------------------------------------------------

/** The signed-in user's record as the settings tabs read and write it. */
export interface ServerSettingsUserRecord {
  id: string;
  email: string;
  firstName: string;
  lastName: string;
  displayName: string;
  avatarUrl: string;
  passwordAutoset: boolean;
}

/** Read the caller's user record (names, email, avatar, password mode). */
export async function serverSettingsUserRecord(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerSettingsUserRecord> {
  const res = await fetchTolerant(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] users/me read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  if (typeof row["id"] !== "string") throw new Error("[parity] users/me row carried no id.");
  const text = (key: string): string => (typeof row[key] === "string" ? (row[key] as string) : "");
  return {
    id: row["id"],
    email: text("email"),
    firstName: text("first_name"),
    lastName: text("last_name"),
    displayName: text("display_name"),
    avatarUrl: text("avatar_url"),
    passwordAutoset: row["is_password_autoset"] === true,
  };
}

/** Instance gates behind the settings account controls (mail, self-managed). */
export async function serverInstanceFlags(
  apiBase: string = apiBaseFromEnv()
): Promise<{ smtpConfigured: boolean; selfManaged: boolean }> {
  const res = await fetchTolerant(`${apiBase}/api/instances/`);
  if (!res.ok) throw new Error(`[parity] instance config read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { config?: Record<string, unknown> };
  const config = payload.config ?? {};
  return {
    smtpConfigured: config["is_smtp_configured"] === true,
    selfManaged: config["is_self_managed"] === true,
  };
}

/**
 * Plant an email-update verification code in the server cache for a user,
 * standing in for the mailed code (the backend binds it to user plus new
 * address for ten minutes). Mirrors mintMagicCode's slot-aware redis path by
 * going through the Django cache API instead, so key formatting always
 * matches the update_email read.
 */
export async function serverMintEmailUpdateCode(userId: string, email: string, code?: string): Promise<string> {
  const value = code ?? String(Math.floor(100000 + Math.random() * 900000));
  const key = `magic_email_update_${userId}_${email}`;
  const out = await apiShell(
    `import json\n` +
      `from django.core.cache import cache\n` +
      `cache.set(${JSON.stringify(key)}, json.dumps({"token": ${JSON.stringify(value)}}), timeout=600)\n` +
      `print("PARITY_EMAIL_CODE_OK")\n`
  );
  if (!out.includes("PARITY_EMAIL_CODE_OK")) {
    throw new Error(`[parity] email-update code mint produced no ack for ${email}.`);
  }
  return value;
}

/** Whether the account with this address is still active (deactivation flips it off). */
export async function serverUserActive(email: string): Promise<boolean> {
  const out = await apiShell(
    `from pi_dash.db.models import User\n` +
      `user = User.objects.filter(email=${JSON.stringify(email)}).first()\n` +
      `print("PARITY_ACTIVE:" + str(bool(user and user.is_active)))\n`
  );
  const line = /^PARITY_ACTIVE:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (line === "") throw new Error(`[parity] user active read produced no row for ${email}.`);
  return line === "True";
}

/**
 * Create an account with a one-time magic code the way the code form does,
 * resolving with a full authenticated session (the sign-in endpoint only
 * serves existing users). The caller plants the code first with
 * mintMagicCode; the sign-up endpoint still requires the instance mail
 * switch, so flip it on for the call and restore it after.
 */
export async function serverMagicSignUp(
  email: string,
  code: string,
  apiBase: string = apiBaseFromEnv()
): Promise<AuthedSession> {
  const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  const preCookies = cookieHeader(setCookieHeaders(tokenRes));
  const body = new URLSearchParams({ email, code, csrfmiddlewaretoken: token });
  const res = await fetchTolerant(`${apiBase}/auth/magic-sign-up/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302) {
    throw new Error(`[parity] magic sign-up failed with HTTP ${res.status} for ${email}.`);
  }
  const cookie = cookieHeader([...setCookieHeaders(tokenRes), ...setCookieHeaders(res)]);
  if (!cookie.includes("session-id=")) throw new Error("[parity] magic sign-up carried no session cookie.");
  const meRes = await fetchTolerant(`${apiBase}/api/users/me/`, { headers: { cookie } });
  if (!meRes.ok) throw new Error(`[parity] users/me read failed with HTTP ${meRes.status} after magic sign-up.`);
  const me = (await meRes.json()) as { id?: unknown };
  const userId = typeof me.id === "string" ? me.id : "";
  const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(cookie)?.[1] ?? "";
  return { email, password: "", userId, cookie, csrfToken, apiBase };
}

// ---------------------------------------------------------------------------
// Member profile reads (NEWFRONT-425, PROF-001..008).
// Server-state twins of what the profile overview, issue tabs, activity tab
// and identity sidebar render: the user-stats, user-profile, user-activity
// and user-issues reads plus an issue-subscribe fixture helper. Appended;
// existing helpers above are untouched per the shared contract.
// ---------------------------------------------------------------------------

/** One user's workspace stats: counts plus priority/state distributions. */
export interface ServerUserStats {
  assigned_issues: number;
  created_issues: number;
  subscribed_issues: number;
  completed_issues: number;
  pending_issues: number;
  priority_distribution: Array<{ priority: string; priority_count: number }>;
  state_distribution: Array<{ state_group: string; state_count: number }>;
}

function serverProfileCount(payload: Record<string, unknown>, field: string): number {
  const value = payload[field];
  if (typeof value !== "number") throw new Error(`[parity] user-stats row carried no numeric ${field}.`);
  return value;
}

/** The user-stats read behind the overview cards and charts. */
export async function serverUserStats(
  session: AuthedSession,
  workspaceSlug: string,
  userId: string
): Promise<ServerUserStats> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${workspaceSlug}/user-stats/${userId}/`);
  if (!res.ok) throw new Error(`[parity] user-stats read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const priorities = payload["priority_distribution"];
  const states = payload["state_distribution"];
  if (!Array.isArray(priorities)) throw new Error("[parity] user-stats row carried no priority distribution.");
  if (!Array.isArray(states)) throw new Error("[parity] user-stats row carried no state distribution.");
  return {
    assigned_issues: serverProfileCount(payload, "assigned_issues"),
    created_issues: serverProfileCount(payload, "created_issues"),
    subscribed_issues: serverProfileCount(payload, "subscribed_issues"),
    completed_issues: serverProfileCount(payload, "completed_issues"),
    pending_issues: serverProfileCount(payload, "pending_issues"),
    priority_distribution: (priorities as Array<Record<string, unknown>>).map((row) => {
      if (typeof row["priority"] !== "string" || typeof row["priority_count"] !== "number") {
        throw new Error("[parity] priority distribution row carried no priority/count.");
      }
      return { priority: row["priority"], priority_count: row["priority_count"] };
    }),
    state_distribution: (states as Array<Record<string, unknown>>).map((row) => {
      if (typeof row["state_group"] !== "string" || typeof row["state_count"] !== "number") {
        throw new Error("[parity] state distribution row carried no group/count.");
      }
      return { state_group: row["state_group"], state_count: row["state_count"] };
    }),
  };
}

/** One user's identity plus per-project breakdown behind the sidebar. */
export interface ServerProfileSegregation {
  project_data: Array<{
    id: string;
    assigned_issues: number;
    completed_issues: number;
    created_issues: number;
    pending_issues: number;
  }>;
  user_data: {
    avatar_url: string | null;
    display_name: string;
    first_name: string;
    last_name: string;
    date_joined: string;
    user_timezone: string;
  };
}

/** The user-profile (segregation) read behind the sidebar and breadcrumb. */
export async function serverProfileSegregation(
  session: AuthedSession,
  workspaceSlug: string,
  userId: string
): Promise<ServerProfileSegregation> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${workspaceSlug}/user-profile/${userId}/`);
  if (!res.ok) throw new Error(`[parity] user-profile read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const projects = payload["project_data"];
  const user = payload["user_data"];
  if (!Array.isArray(projects)) throw new Error("[parity] user-profile row carried no project data.");
  if (typeof user !== "object" || user === null) throw new Error("[parity] user-profile row carried no user data.");
  const userRow = user as Record<string, unknown>;
  for (const field of ["display_name", "first_name", "last_name", "date_joined", "user_timezone"]) {
    if (typeof userRow[field] !== "string") throw new Error(`[parity] user-profile user row carried no ${field}.`);
  }
  return {
    project_data: (projects as Array<Record<string, unknown>>).map((row) => {
      if (typeof row["id"] !== "string") throw new Error("[parity] segregation project row carried no id.");
      for (const field of ["assigned_issues", "completed_issues", "created_issues", "pending_issues"]) {
        if (typeof row[field] !== "number") throw new Error(`[parity] segregation project row carried no ${field}.`);
      }
      return {
        id: row["id"] as string,
        assigned_issues: row["assigned_issues"] as number,
        completed_issues: row["completed_issues"] as number,
        created_issues: row["created_issues"] as number,
        pending_issues: row["pending_issues"] as number,
      };
    }),
    user_data: {
      avatar_url: typeof userRow["avatar_url"] === "string" ? (userRow["avatar_url"] as string) : null,
      display_name: userRow["display_name"] as string,
      first_name: userRow["first_name"] as string,
      last_name: userRow["last_name"] as string,
      date_joined: userRow["date_joined"] as string,
      user_timezone: userRow["user_timezone"] as string,
    },
  };
}

/** One user-activity feed entry (id plus actor display name). */
export interface ServerUserActivityEntry {
  id: string;
  actor: string | null;
}

/** A user-activity feed page: entries plus paging totals. */
export interface ServerUserActivity {
  results: ServerUserActivityEntry[];
  total_pages: number;
  total_results: number;
}

/** The user-activity read behind the overview section and the activity tab. */
export async function serverUserActivity(
  session: AuthedSession,
  workspaceSlug: string,
  userId: string,
  query: { per_page: number; cursor?: string }
): Promise<ServerUserActivity> {
  const params = new URLSearchParams({ per_page: String(query.per_page) });
  if (query.cursor !== undefined) params.set("cursor", query.cursor);
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${workspaceSlug}/user-activity/${userId}/?${params.toString()}`
  );
  if (!res.ok) throw new Error(`[parity] user-activity read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  if (!Array.isArray(payload["results"])) throw new Error("[parity] user-activity row carried no results.");
  if (typeof payload["total_pages"] !== "number" || typeof payload["total_results"] !== "number") {
    throw new Error("[parity] user-activity row carried no paging totals.");
  }
  return {
    results: (payload["results"] as Array<Record<string, unknown>>).map((row) => {
      if (typeof row["id"] !== "string") throw new Error("[parity] user-activity entry carried no id.");
      const actor = row["actor_detail"] as Record<string, unknown> | undefined;
      return {
        id: row["id"],
        actor: typeof actor?.["display_name"] === "string" ? (actor["display_name"] as string) : null,
      };
    }),
    total_pages: payload["total_pages"] as number,
    total_results: payload["total_results"] as number,
  };
}

/** One user-issues row (id plus name). */
export interface ServerUserIssue {
  id: string;
  name: string;
}

function serverUserIssueOf(row: Record<string, unknown>): ServerUserIssue {
  if (typeof row["id"] !== "string" || typeof row["name"] !== "string") {
    throw new Error("[parity] user-issues row carried no id/name.");
  }
  return { id: row["id"], name: row["name"] };
}

/**
 * The user-issues read behind one issue tab. Pass the tab's own filter
 * (`assignees`, `created_by` or `subscriber` set to the viewed user); the
 * payload flattens grouped and flat shapes alike.
 */
export async function serverUserIssues(
  session: AuthedSession,
  workspaceSlug: string,
  userId: string,
  params: Record<string, string>
): Promise<ServerUserIssue[]> {
  const query = new URLSearchParams(params).toString();
  const res = await sessionFetch(session, "GET", `/api/workspaces/${workspaceSlug}/user-issues/${userId}/?${query}`);
  if (!res.ok) throw new Error(`[parity] user-issues read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  const results = payload["results"];
  if (Array.isArray(results)) {
    return (results as Array<Record<string, unknown>>).map(serverUserIssueOf);
  }
  if (typeof results === "object" && results !== null) {
    const out: ServerUserIssue[] = [];
    for (const group of Object.values(results as Record<string, unknown>)) {
      const rows = (group as Record<string, unknown>)["results"];
      if (Array.isArray(rows)) {
        for (const row of rows as Array<Record<string, unknown>>) out.push(serverUserIssueOf(row));
      }
    }
    return out;
  }
  throw new Error("[parity] user-issues row carried no results.");
}

/** Subscribe the session owner to an issue (fixture setup for the subscribed tab). */
export async function serverSubscribeIssue(
  session: AuthedSession,
  workspaceSlug: string,
  projectId: string,
  issueId: string
): Promise<void> {
  const res = await sessionFetch(
    session,
    "POST",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/subscribe/`
  );
  if (!res.ok) throw new Error(`[parity] issue subscribe failed with HTTP ${res.status}.`);
}

/**
 * Set the session owner's avatar through the same signed-URL flow the
 * profile form uses; the status flip links the asset to the user, which
 * is what the sidebar avatar URL derives from. Resolves with the asset
 * id plus its served URL.
 */
export async function serverUploadUserAvatar(
  session: AuthedSession,
  file: { name: string; mime: string; bytes: Uint8Array }
): Promise<{ assetId: string; assetUrl: string }> {
  const created = await sessionFetch(session, "POST", `/api/assets/v2/user-assets/`, {
    name: file.name,
    type: file.mime,
    size: file.bytes.length,
    entity_type: "USER_AVATAR",
    entity_identifier: "",
  });
  if (!created.ok) throw new Error(`[parity] avatar asset row create failed with HTTP ${created.status}.`);
  const signed = (await created.json()) as {
    asset_id: string;
    asset_url: string;
    upload_data: { url: string; fields: Record<string, string> };
  };
  if (typeof signed.asset_id !== "string" || typeof signed.upload_data?.url !== "string") {
    throw new Error("[parity] avatar asset row create returned no signed URL.");
  }
  const form = new FormData();
  for (const [key, value] of Object.entries(signed.upload_data.fields ?? {})) form.append(key, value);
  form.append("file", new Blob([file.bytes], { type: file.mime }), file.name);
  // The server signs the upload against the request host, so a direct API
  // call yields an API-origin URL that Django cannot serve (404); only the
  // parity proxy routes the bucket path to minio. Re-host the signed URL
  // there — the signature covers the fields, not the host.
  const uploadUrl = (() => {
    try {
      const proxy = new URL(parityOracleUrl(process.env));
      const target = new URL(signed.upload_data.url);
      target.protocol = proxy.protocol;
      target.host = proxy.host;
      return target.toString();
    } catch {
      return signed.upload_data.url;
    }
  })();
  // Object storage is stubbed on the shared runners (the minio image
  // resolves to `sleep infinity`, so the proxy answers 502); the asset
  // record and the avatar link the scenario asserts are pure database
  // rows and verify identically without bytes. Upload bytes when the
  // store answers, but only a dead store falls back to record-only.
  let stored: Response | null = null;
  try {
    stored = await fetch(uploadUrl, { method: "POST", body: form });
  } catch {
    stored = null;
  }
  if (stored === null || stored.status === 502 || stored.status === 503 || stored.status === 504) {
    console.warn(
      `[parity] object store unreachable (status ${stored === null ? "refused" : stored.status}); continuing record-only.`
    );
  } else if (!stored.ok) {
    throw new Error(`[parity] avatar byte upload failed with HTTP ${stored.status}.`);
  }
  const flipped = await sessionFetch(session, "PATCH", `/api/assets/v2/user-assets/${signed.asset_id}/`, {});
  if (!flipped.ok) throw new Error(`[parity] avatar asset status flip failed with HTTP ${flipped.status}.`);
  return {
    assetId: signed.asset_id,
    assetUrl: typeof signed.asset_url === "string" ? signed.asset_url : "",
  };
}

// --- Space layouts & cards oracle (NEWFRONT-449, SPC-020..033).
// --- Appended; existing helpers above are untouched per the shared driver
// --- contract.

/** One public-issues group as the board fetch returns it. */
export interface ServerPublicIssueGroup {
  id: string;
  total: number;
  nextPageResults: boolean;
  nextCursor: string;
  issues: Array<{ id: string; name: string; sequenceId: number; stateId: string; priority: string }>;
}

/**
 * Grouped public-issues read in the shape the board's first fetch uses
 * (cursor/per_page/group_by). The session cookie is optional; specs pass
 * the seed-owner session so reads ride the authed throttle bucket while
 * the browser stays an anonymous visitor.
 */
export async function serverPublicIssues(
  anchor: string,
  params: Record<string, string> = {},
  sessionCookie?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ totalCount: number; groups: ServerPublicIssueGroup[] }> {
  const query = new URLSearchParams({ cursor: "50:0:0", per_page: "50", group_by: "state_id", ...params });
  const headers: Record<string, string> = {};
  if (sessionCookie !== undefined) headers["cookie"] = sessionCookie;
  const res = await fetch(`${apiBase}/api/public/anchor/${anchor}/issues/?${query}`, { headers });
  if (!res.ok) throw new Error(`[parity] public issues read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as Record<string, unknown>;
  const totalCount = body["total_count"];
  if (typeof totalCount !== "number") throw new Error("[parity] public issues carried no numeric total_count.");
  const results = body["results"];
  if (typeof results !== "object" || results === null || Array.isArray(results))
    throw new Error("[parity] public issues carried no grouped results.");
  const groups: ServerPublicIssueGroup[] = [];
  for (const [id, raw] of Object.entries(results as Record<string, unknown>)) {
    const group = raw as Record<string, unknown>;
    const total = group["total_results"];
    const rows = group["results"];
    if (typeof total !== "number" || !Array.isArray(rows))
      throw new Error("[parity] public issue group carried no total/results.");
    groups.push({
      id,
      total,
      nextPageResults: group["next_page_results"] === true,
      nextCursor: typeof group["next_cursor"] === "string" ? (group["next_cursor"] as string) : "",
      issues: rows.map((row) => {
        const issue = row as Record<string, unknown>;
        const issueId = issue["id"];
        const name = issue["name"];
        const sequenceId = issue["sequence_id"];
        const stateId = issue["state_id"];
        const priority = issue["priority"];
        if (
          typeof issueId !== "string" ||
          typeof name !== "string" ||
          typeof sequenceId !== "number" ||
          typeof stateId !== "string" ||
          typeof priority !== "string"
        )
          throw new Error("[parity] public issue carried no id/name/sequence/state/priority.");
        return { id: issueId, name, sequenceId, stateId, priority };
      }),
    });
  }
  return { totalCount, groups };
}

/** Public board settings the app reads for flags and views. */
export interface ServerPublicBoardSettings {
  viewProps: Record<string, unknown>;
  isVotesEnabled: boolean;
  isReactionsEnabled: boolean;
  isCommentsEnabled: boolean;
}

/** Public settings read; verifies board flips landed server-side. */
export async function serverPublicBoardSettings(
  anchor: string,
  sessionCookie?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerPublicBoardSettings> {
  const headers: Record<string, string> = {};
  if (sessionCookie !== undefined) headers["cookie"] = sessionCookie;
  const res = await fetch(`${apiBase}/api/public/anchor/${anchor}/settings/`, { headers });
  if (!res.ok) throw new Error(`[parity] public settings read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  const viewProps = rec["view_props"];
  if (typeof viewProps !== "object" || viewProps === null)
    throw new Error("[parity] public settings carried no view_props.");
  return {
    viewProps: viewProps as Record<string, unknown>,
    isVotesEnabled: rec["is_votes_enabled"] === true,
    isReactionsEnabled: rec["is_reactions_enabled"] === true,
    isCommentsEnabled: rec["is_comments_enabled"] === true,
  };
}

/** Public states for group-name mapping. */
export async function serverPublicStates(
  anchor: string,
  sessionCookie?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Array<{ id: string; name: string; group: string }>> {
  const headers: Record<string, string> = {};
  if (sessionCookie !== undefined) headers["cookie"] = sessionCookie;
  const res = await fetch(`${apiBase}/api/public/anchor/${anchor}/states/`, { headers });
  if (!res.ok) throw new Error(`[parity] public states read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Record<string, unknown>[];
  return rows.map((row) => {
    const { id, name, group } = row;
    if (typeof id !== "string" || typeof name !== "string" || typeof group !== "string")
      throw new Error("[parity] public state carried no string id/name/group.");
    return { id, name, group };
  });
}

/** Flip a published board's views/flags; specs restore the previous shape after. */
export async function serverPatchBoard(
  workspaceSlug: string,
  projectId: string,
  boardId: string,
  patch: Record<string, unknown>,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/${boardId}/`,
    sessionCookie,
    patch
  );
  if (!res.ok) throw new Error(`[parity] board patch failed with HTTP ${res.status}.`);
}

// --- Instance-admin entry/session oracle (NEWFRONT-441, ADM-001..012).
// --- Session/setup helpers for the admin console specs. Appended;
// --- existing helpers above are untouched per the shared harness contract.

/** Instance facts the admin bootstrap gate branches on. */
export interface ServerAdminInstanceInfo {
  isSetupDone: boolean;
  workspacesExist: boolean;
  instanceName: string;
  telemetryEnabled: boolean;
  /** Stable instance identifier (general-settings read-only field). */
  instanceId: string;
}

/** The stack's api container: explicit override, else derived from PARITY_NS. */
function adminApiContainer(): string {
  const override = (process.env["PARITY_API_CONTAINER"] ?? "").trim();
  if (override !== "") return override;
  return `${resolveParityEnv(process.env).containerPrefix}-api`;
}

/** Run a Django shell snippet inside the stack's api container; resolves with stdout. */
async function serverAdminShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", adminApiContainer(), "python", "manage.py", "shell", "-c", python],
    { timeout: 120_000 }
  );
  return stdout;
}

/** Instance info as GET /api/instances/ reports it (anonymous read, like the gate). */
export async function serverAdminInstanceInfo(apiBase: string = apiBaseFromEnv()): Promise<ServerAdminInstanceInfo> {
  const res = await fetchTolerant(`${apiBase}/api/instances/`);
  if (!res.ok) throw new Error(`[parity] instance info read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as { instance?: Record<string, unknown> };
  const row = body.instance ?? {};
  if (typeof row["is_setup_done"] !== "boolean" || typeof row["workspaces_exist"] !== "boolean") {
    throw new Error("[parity] instance info carried no setup flag and workspaces flag.");
  }
  return {
    isSetupDone: row["is_setup_done"],
    workspacesExist: row["workspaces_exist"],
    instanceName: typeof row["instance_name"] === "string" ? row["instance_name"] : "",
    telemetryEnabled: row["is_telemetry_enabled"] === true,
    instanceId: typeof row["instance_id"] === "string" ? row["instance_id"] : "",
  };
}

/**
 * Flip the instance setup flag for one scenario; resolves with the
 * previous value for restore. The scratch stack runs with DEBUG, so no
 * server cache holds the old flag — the delete below is a belt for
 * non-debug stacks, where GET /api/instances/ caches for two hours.
 */
export async function serverAdminSetSetupDone(done: boolean): Promise<boolean> {
  const out = await serverAdminShell(
    `from pi_dash.license.models import Instance\n` +
      `from django.core.cache import cache\n` +
      `instance = Instance.objects.first()\n` +
      `print("PARITY_WAS:" + ("1" if instance.is_setup_done else "0"))\n` +
      `instance.is_setup_done = ${done ? "True" : "False"}\n` +
      `instance.save(update_fields=["is_setup_done"])\n` +
      `cache.delete("/api/instances/")\n` +
      `print("PARITY_SETUP_OK")\n`
  );
  if (!out.includes("PARITY_SETUP_OK")) throw new Error("[parity] setup-flag flip produced no receipt.");
  return /^PARITY_WAS:1$/m.test(out);
}

/** Emails holding an instance-admin row, in server order. */
export async function serverAdminListAdmins(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetch(`${apiBase}/api/instances/admins/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] instance-admin list failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ user_detail?: { email?: unknown } }>;
  return rows.map((row) => row.user_detail?.email).filter((email): email is string => typeof email === "string");
}

/** Admin sign-in through the native credential endpoint; resolves with a session cookie header. */
export async function serverAdminSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  return withThrottleRetry(`admin sign-in ${email}`, async () => {
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
    // Both outcomes redirect: success lands on general/, failures carry
    // error_code params back at the entry — the Location tells them apart.
    const location = res.headers.get("location") ?? "";
    if (res.status !== 302 || !location.includes("general/")) {
      throw new Error(`[parity] admin sign-in for ${email} landed on ${JSON.stringify(location)}.`);
    }
    const header = cookieHeader([...setCookieHeaders(tokenRes), ...setCookieHeaders(res)]);
    if (header === "") throw new Error("[parity] admin sign-in response carried no session cookie.");
    return header;
  });
}

/** Admin identity check: status plus email when the session is an admin session. */
export async function serverAdminMe(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; email: string | null }> {
  const res = await fetch(`${apiBase}/api/instances/admins/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) return { status: res.status, email: null };
  const row = (await res.json()) as { email?: unknown };
  return { status: res.status, email: typeof row.email === "string" ? row.email : null };
}

/**
 * Clear every instance-admin row and mark setup un-done, for the setup
 * scenarios; resolves with the previous admin count and setup flag so
 * the scenario restores both in teardown.
 */
export async function serverAdminClearForSetup(): Promise<{ adminCount: number; wasSetupDone: boolean }> {
  const out = await serverAdminShell(
    `from pi_dash.license.models import Instance, InstanceAdmin\n` +
      `from django.core.cache import cache\n` +
      `count = InstanceAdmin.objects.count()\n` +
      `# Hard-delete through all_objects: the default manager soft-deletes,\n` +
      `# and the unique (instance, user) row would block the re-grant.\n` +
      `InstanceAdmin.all_objects.all().delete()\n` +
      `instance = Instance.objects.first()\n` +
      `print("PARITY_COUNT:" + str(count))\n` +
      `print("PARITY_WAS:" + ("1" if instance.is_setup_done else "0"))\n` +
      `instance.is_setup_done = False\n` +
      `instance.save(update_fields=["is_setup_done"])\n` +
      `cache.delete("/api/instances/")\n` +
      `print("PARITY_CLEAR_OK")\n`
  );
  if (!out.includes("PARITY_CLEAR_OK")) throw new Error("[parity] setup clear produced no receipt.");
  const count = Number.parseInt(/^PARITY_COUNT:(\d+)$/m.exec(out)?.[1] ?? "", 10);
  if (!Number.isInteger(count)) throw new Error("[parity] setup clear reported no admin count.");
  return { adminCount: count, wasSetupDone: /^PARITY_WAS:1$/m.test(out) };
}

/**
 * Restore the seeded admin world after a setup scenario: drop the admin
 * row and user the scenario created, re-grant the seed owner when the
 * clear removed rows, and restore the setup flag plus the instance name
 * and telemetry flag the setup POST overwrote. Best-effort per step so
 * teardown never masks the scenario failure; logs instead of throwing.
 */
export async function serverAdminRestoreAfterSetup(
  seedEmail: string,
  createdEmail: string,
  previous: { adminCount: number; wasSetupDone: boolean; instanceName: string; telemetryEnabled: boolean }
): Promise<void> {
  try {
    const out = await serverAdminShell(
      `from pi_dash.license.models import Instance, InstanceAdmin\n` +
        `from pi_dash.db.models import User\n` +
        `from django.core.cache import cache\n` +
        `# Hard-delete through all_objects (soft-deleted rows keep the unique key).\n` +
        `InstanceAdmin.all_objects.filter(user__email=${JSON.stringify(createdEmail)}).delete()\n` +
        `User.objects.filter(email=${JSON.stringify(createdEmail)}).delete()\n` +
        `instance = Instance.objects.first()\n` +
        `owner = User.objects.filter(email=${JSON.stringify(seedEmail)}).first()\n` +
        `if owner is not None and ${previous.adminCount > 0 ? "True" : "False"}:\n` +
        `    row = InstanceAdmin.all_objects.filter(instance=instance, user=owner).first()\n` +
        `    if row is None:\n` +
        `        InstanceAdmin.objects.create(instance=instance, user=owner, role=20, is_verified=True)\n` +
        `    else:\n` +
        `        row.deleted_at = None\n` +
        `        row.role = 20\n` +
        `        row.is_verified = True\n` +
        `        row.save(update_fields=["deleted_at", "role", "is_verified"])\n` +
        `instance.is_setup_done = ${previous.wasSetupDone ? "True" : "False"}\n` +
        `instance.instance_name = ${JSON.stringify(previous.instanceName)}\n` +
        `instance.is_telemetry_enabled = ${previous.telemetryEnabled ? "True" : "False"}\n` +
        `instance.save(update_fields=["is_setup_done", "instance_name", "is_telemetry_enabled"])\n` +
        `cache.delete("/api/instances/")\n` +
        `print("PARITY_RESTORE_OK")\n`
    );
    if (!out.includes("PARITY_RESTORE_OK"))
      console.log("[parity] setup restore produced no receipt; leaving it for reseed.");
  } catch (error) {
    console.log(
      `[parity] setup restore failed (${error instanceof Error ? error.message : String(error)}); leaving it for reseed.`
    );
  }
}

/**
 * Grant or revoke one user's instance-admin row (verified, role 20 when
 * granted). Revocation hard-deletes through all_objects so the unique
 * (instance, user) key is freed; granting undeletes-or-creates so
 * soft-deleted leftovers never block the re-grant.
 */
export async function serverAdminSetGrant(email: string, granted: boolean): Promise<void> {
  const verb = granted ? "grant" : "revoke";
  const out = await serverAdminShell(
    `from pi_dash.license.models import Instance, InstanceAdmin\n` +
      `from pi_dash.db.models import User\n` +
      `instance = Instance.objects.first()\n` +
      `owner = User.objects.filter(email=${JSON.stringify(email)}).first()\n` +
      `if owner is None:\n` +
      `    raise SystemExit("PARITY_NO_USER")\n` +
      (granted
        ? `row = InstanceAdmin.all_objects.filter(instance=instance, user=owner).first()\n` +
          `if row is None:\n` +
          `    InstanceAdmin.objects.create(instance=instance, user=owner, role=20, is_verified=True)\n` +
          `else:\n` +
          `    row.deleted_at = None\n` +
          `    row.role = 20\n` +
          `    row.is_verified = True\n` +
          `    row.save(update_fields=["deleted_at", "role", "is_verified"])\n`
        : `InstanceAdmin.all_objects.filter(instance=instance, user=owner).delete()\n`) +
      `print("PARITY_GRANT_OK")\n`
  );
  if (!out.includes("PARITY_GRANT_OK")) throw new Error(`[parity] admin ${verb} for ${email} produced no receipt.`);
}

/** Admin sign-out through the native endpoint; the session reads as signed out after. */
export async function serverAdminSignOut(sessionCookie: string, apiBase: string = apiBaseFromEnv()): Promise<void> {
  const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`, { headers: { cookie: sessionCookie } });
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  // The session header rides through verbatim (re-joining it would keep
  // only its first pair and drop the admin session), with the fresh
  // CSRF cookie last: duplicate cookie keys resolve to the last value,
  // which must match the posted token.
  const fresh = cookieHeader(setCookieHeaders(tokenRes));
  const cookies = sessionCookie === "" ? fresh : `${sessionCookie}; ${fresh}`;
  const body = new URLSearchParams({ csrfmiddlewaretoken: token });
  const res = await fetchTolerant(`${apiBase}/api/instances/admins/sign-out/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: cookies },
    body,
    redirect: "manual",
  });
  if (res.status !== 302) throw new Error(`[parity] admin sign-out answered HTTP ${res.status}, not a redirect.`);
}

// --- Space board access & routing server reads (NEWFRONT-447, SPC-001..009).
// --- Public space endpoints need no session; the publish upsert/delete
// --- helpers below run as the seed admin and restore after.

/** Raw public-endpoint read: HTTP status plus the decoded JSON body (or null). */
export interface ServerSpacePublicRead {
  status: number;
  payload: unknown;
}

async function serverSpacePublicGet(path: string, apiBase: string): Promise<ServerSpacePublicRead> {
  const res = await fetchTolerant(`${apiBase}${path}`);
  const text = await res.text();
  let payload: unknown = null;
  try {
    payload = text === "" ? null : (JSON.parse(text) as unknown);
  } catch {
    payload = text;
  }
  return { status: res.status, payload };
}

/** Public board settings for an anchor (anonymous; 404 when unpublished). */
export async function serverSpaceBoardSettings(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerSpacePublicRead> {
  return serverSpacePublicGet(`/api/public/anchor/${anchor}/settings/`, apiBase);
}

/** Public board metadata for an anchor (anonymous; 404 when unpublished). */
export async function serverSpaceBoardMeta(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerSpacePublicRead> {
  return serverSpacePublicGet(`/api/public/anchor/${anchor}/meta/`, apiBase);
}

/** Published anchor for a workspace project (anonymous; 404 when unpublished). */
export async function serverSpaceAnchorForProject(
  workspaceSlug: string,
  projectId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerSpacePublicRead> {
  return serverSpacePublicGet(`/api/public/workspaces/${workspaceSlug}/projects/${projectId}/anchor/`, apiBase);
}

/** Names of the issues the public board endpoint serves for an anchor. */
export async function serverSpaceBoardIssueNames(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; names: string[]; ids: string[]; totalCount: number }> {
  const read = await serverSpacePublicGet(`/api/public/anchor/${anchor}/issues/`, apiBase);
  const payload = (read.payload ?? {}) as { results?: unknown; total_count?: unknown };
  const rows = Array.isArray(payload.results) ? payload.results : [];
  const names = rows
    .map((row) => (row as { name?: unknown }).name)
    .filter((name): name is string => typeof name === "string");
  const ids = rows.map((row) => (row as { id?: unknown }).id).filter((id): id is string => typeof id === "string");
  return {
    status: read.status,
    names,
    ids,
    totalCount: typeof payload.total_count === "number" ? payload.total_count : names.length,
  };
}

/** Publish (upsert) a project's board; resolves with the board row id and anchor. */
export async function serverSpacePublishBoard(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; anchor: string }> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/`,
    {
      method: "POST",
      headers: { cookie: sessionCookie, "content-type": "application/json" },
      body: JSON.stringify({}),
    }
  );
  const body = (await res.text().then(
    (text) => (text === "" ? null : (JSON.parse(text) as unknown)),
    () => null
  )) as { id?: unknown; anchor?: unknown } | null;
  if (!res.ok || !body || typeof body.id !== "string" || typeof body.anchor !== "string") {
    throw new Error(`[parity] board publish failed with HTTP ${res.status}.`);
  }
  return { id: body.id, anchor: body.anchor };
}

/** Withdraw a published board; throws unless the server accepts. */
export async function serverSpaceDeleteBoard(
  workspaceSlug: string,
  projectId: string,
  boardId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/${boardId}/`,
    { method: "DELETE", headers: { cookie: sessionCookie } }
  );
  if (res.status !== 200 && res.status !== 204) {
    throw new Error(`[parity] board delete failed with HTTP ${res.status}.`);
  }
}

/** HTTP status of the current-user fetch with (or without) a session cookie. */
export async function serverSpaceMeStatus(
  sessionCookie: string | null,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetchTolerant(`${apiBase}/api/users/me/`, {
    headers: sessionCookie === null ? {} : { cookie: sessionCookie },
  });
  return res.status;
}

// --- Space filters + peek shell (NEWFRONT-450, SPC-034–046). Appended;
// --- existing helpers above are untouched per the shared harness contract.
// --- Public-anchor reads ride anonymous (AllowAny); provisioning and board
// --- flips ride the seed owner's session.

/** One public-issues row, as the scenarios assert on it. */
export interface SpaceIssueFacts {
  id: string;
  name: string;
  sequenceId: number;
  priority: string;
  stateId: string;
  targetDate: string | null;
  descriptionHtml: string | null;
  labelIds: string[];
}

function spaceIssueFacts(row: unknown): SpaceIssueFacts {
  const record = row as {
    id?: unknown;
    name?: unknown;
    sequence_id?: unknown;
    priority?: unknown;
    state_id?: unknown;
    target_date?: unknown;
    description_html?: unknown;
    label_ids?: unknown;
  };
  if (
    typeof record.id !== "string" ||
    typeof record.name !== "string" ||
    typeof record.sequence_id !== "number" ||
    typeof record.priority !== "string" ||
    typeof record.state_id !== "string"
  ) {
    throw new Error("[parity] public issue row carried no id/name/sequence/priority/state.");
  }
  return {
    id: record.id,
    name: record.name,
    sequenceId: record.sequence_id,
    priority: record.priority,
    stateId: record.state_id,
    targetDate: typeof record.target_date === "string" ? record.target_date : null,
    descriptionHtml: typeof record.description_html === "string" ? record.description_html : null,
    labelIds: Array.isArray(record.label_ids)
      ? record.label_ids.filter((id): id is string => typeof id === "string")
      : [],
  };
}

/** Anonymous GET against the public anchor tree; rides out throttle minutes. */
async function spacePublicGet(anchor: string, path: string, apiBase: string = apiBaseFromEnv()): Promise<unknown> {
  return withThrottleRetry(`space public ${path}`, async () => {
    const res = await fetchTolerant(`${apiBase}/api/public/anchor/${anchor}${path}`);
    if (!res.ok) throw new Error(`[parity] space public ${path} failed with HTTP ${res.status}.`);
    return (await res.json()) as unknown;
  });
}

/** Public-issues list rows for an anchor, optionally narrowed by query params. */
export async function serverSpaceIssues(
  anchor: string,
  params: Record<string, string> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceIssueFacts[]> {
  const query = new URLSearchParams(params).toString();
  const payload = await spacePublicGet(anchor, `/issues/${query === "" ? "" : `?${query}`}`, apiBase);
  const results = (payload as { results?: unknown }).results;
  if (!Array.isArray(results)) throw new Error("[parity] public issues response carried no results list.");
  return results.map(spaceIssueFacts);
}

/** One public issue's detail row. */
export async function serverSpaceIssueDetail(
  anchor: string,
  issueId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceIssueFacts> {
  return spaceIssueFacts(await spacePublicGet(anchor, `/issues/${issueId}/`, apiBase));
}

/** Public workflow states backing the state facet. */
export async function serverSpaceStates(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Array<{ id: string; name: string; group: string }>> {
  const rows: unknown = await spacePublicGet(anchor, "/states/", apiBase);
  if (!Array.isArray(rows)) throw new Error("[parity] public states response was not a list.");
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown; group?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.group !== "string") {
      throw new Error("[parity] public state row carried no id/name/group.");
    }
    return { id: record.id, name: record.name, group: record.group };
  });
}

/** Public project labels backing the address-only label filter. */
export async function serverSpaceLabels(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Array<{ id: string; name: string }>> {
  const rows: unknown = await spacePublicGet(anchor, "/labels/", apiBase);
  if (!Array.isArray(rows)) throw new Error("[parity] public labels response was not a list.");
  return rows.map((row) => {
    const record = row as { id?: unknown; name?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string") {
      throw new Error("[parity] public label row carried no id/name.");
    }
    return { id: record.id, name: record.name };
  });
}

/** Published-board settings: engagement flags plus enabled views. */
export async function serverSpaceSettings(
  anchor: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{
  comments: boolean;
  reactions: boolean;
  votes: boolean;
  views: Record<string, boolean>;
}> {
  const record = (await spacePublicGet(anchor, "/settings/", apiBase)) as {
    is_comments_enabled?: unknown;
    is_reactions_enabled?: unknown;
    is_votes_enabled?: unknown;
    view_props?: unknown;
  };
  const views: Record<string, boolean> = {};
  if (record.view_props !== null && typeof record.view_props === "object") {
    for (const [key, value] of Object.entries(record.view_props as Record<string, unknown>)) {
      views[key] = value === true;
    }
  }
  return {
    comments: record.is_comments_enabled === true,
    reactions: record.is_reactions_enabled === true,
    votes: record.is_votes_enabled === true,
    views,
  };
}

/** Fields the scenarios set when provisioning probe issues. */
export interface SpaceProbeIssue {
  name: string;
  priority?: string;
  targetDate?: string;
  descriptionHtml?: string;
  labelIds?: string[];
}

/** Provision a probe issue; resolves with its id (caller deletes it after). */
export async function serverSpaceCreateIssue(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  probe: SpaceProbeIssue,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const body: Record<string, unknown> = { name: probe.name };
  if (probe.priority !== undefined) body["priority"] = probe.priority;
  if (probe.targetDate !== undefined) body["target_date"] = probe.targetDate;
  if (probe.descriptionHtml !== undefined) body["description_html"] = probe.descriptionHtml;
  if (probe.labelIds !== undefined) body["label_ids"] = probe.labelIds;
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  const payload = (await res.json().catch(() => null)) as { id?: unknown } | null;
  if ((res.status !== 201 && res.status !== 200) || payload === null || typeof payload.id !== "string") {
    throw new Error(`[parity] probe issue create failed with HTTP ${res.status}: ${JSON.stringify(payload)}`);
  }
  return payload.id;
}

/** Delete every project issue whose name starts with the probe prefix. */
export async function serverSpaceCleanupProbes(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  prefix: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  for (const issue of await serverIssues(workspaceSlug, projectId, sessionCookie, apiBase)) {
    if (issue.name.startsWith(prefix))
      await serverDeleteIssue(workspaceSlug, projectId, issue.id, sessionCookie, apiBase);
  }
}

/** Delete every project label whose name starts with the probe prefix. */
export async function serverSpaceCleanupProbeLabels(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  prefix: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  for (const label of await projectLabels(workspaceSlug, projectId, sessionCookie, apiBase)) {
    if (label.name.startsWith(prefix))
      await deleteProjectLabel(workspaceSlug, projectId, label.id, sessionCookie, apiBase);
  }
}

/** Flip the published board's engagement flags (read-modify-write: the upsert replaces every flag). */
export async function serverSpaceSetBoardFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  patch: { comments?: boolean; reactions?: boolean; votes?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const current = await serverSpaceSettingsSeedRead(workspaceSlug, projectId, sessionCookie, apiBase);
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/project-deploy-boards/`,
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        is_comments_enabled: patch.comments ?? current.comments,
        is_reactions_enabled: patch.reactions ?? current.reactions,
        is_votes_enabled: patch.votes ?? current.votes,
        views: current.views,
      }),
    },
    apiBase
  );
  if (!res.ok) throw new Error(`[parity] deploy-board flag flip failed with HTTP ${res.status}.`);
}

/** Current board flags plus views through the workspace API (the flip's read half). */
async function serverSpaceSettingsSeedRead(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string
): Promise<{ comments: boolean; reactions: boolean; votes: boolean; views: Record<string, boolean> }> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/project-deploy-boards/`,
    sessionCookie,
    undefined,
    apiBase
  );
  requireOk(res, "deploy-board read");
  const record = (await res.json()) as {
    is_comments_enabled?: unknown;
    is_reactions_enabled?: unknown;
    is_votes_enabled?: unknown;
    view_props?: unknown;
  };
  const views: Record<string, boolean> = {};
  if (record.view_props !== null && typeof record.view_props === "object") {
    for (const [key, value] of Object.entries(record.view_props as Record<string, unknown>)) {
      views[key] = value === true;
    }
  }
  return {
    comments: record.is_comments_enabled === true,
    reactions: record.is_reactions_enabled === true,
    votes: record.is_votes_enabled === true,
    views,
  };
}

// --- Spaces auth helpers (NEWFRONT-452, SPC-058..064). Appended; existing
// --- helpers above are untouched per the shared harness contract. The
// --- spaces endpoints mirror the app auth endpoints with their own URL
// --- prefix and redirect base, so these are small spaces-shaped variants
// --- of emailCheck/magicGenerate/nativeMagicSignIn/signUpAccount.

/**
 * Raw spaces email-check call; throws carrying the server's error_code on
 * failure. Always anonymous: this endpoint enforces CSRF for
 * session-authenticated callers, so sending a session cookie would 403.
 */
export async function spaceEmailCheck(email: string, apiBase: string = apiBaseFromEnv()): Promise<EmailCheckResult> {
  const res = await fetchTolerant(`${apiBase}/auth/spaces/email-check/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email }),
  });
  const payload: unknown = await res.json();
  if (!res.ok) {
    const code = (payload as { error_code?: unknown }).error_code;
    throw new Error(`[parity] spaces email-check failed with HTTP ${res.status} (error_code ${String(code)}).`);
  }
  return payload as EmailCheckResult;
}

/** Generate a spaces one-time code for an address; resolves with the key. */
export async function spaceMagicGenerate(email: string, apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetchTolerant(`${apiBase}/auth/spaces/magic-generate/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email }),
  });
  const payload: unknown = await res.json();
  if (!res.ok) {
    const code = (payload as { error_code?: unknown }).error_code;
    throw new Error(`[parity] spaces magic-generate failed with HTTP ${res.status} (error_code ${String(code)}).`);
  }
  const key = (payload as { key?: unknown }).key;
  if (typeof key !== "string") throw new Error("[parity] spaces magic-generate response carried no key.");
  return key;
}

/** Outcome of posting a spaces native form straight at the server. */
export interface NativeSpaceAuthResult {
  status: number;
  location: string;
  sessionCookie: boolean;
}

/** Shared native-POST plumbing for the spaces auth forms (form outcomes never throw). */
async function nativeSpacePost(
  path: string,
  fields: Record<string, string>,
  apiBase: string
): Promise<NativeSpaceAuthResult> {
  const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  const preCookies = cookieHeader(setCookieHeaders(tokenRes));
  const body = new URLSearchParams({ ...fields, csrfmiddlewaretoken: token });
  const res = await fetchTolerant(`${apiBase}${path}`, {
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

/** Submit a spaces one-time code the way the code form does; never throws. */
export async function nativeSpaceMagicAuth(
  email: string,
  code: string,
  mode: "sign-in" | "sign-up",
  apiBase: string = apiBaseFromEnv()
): Promise<NativeSpaceAuthResult> {
  return nativeSpacePost(`/auth/spaces/magic-${mode}/`, { email, code }, apiBase);
}

/** Submit spaces credentials the way the password form does; never throws. */
export async function nativeSpacePasswordAuth(
  email: string,
  password: string,
  mode: "sign-in" | "sign-up",
  apiBase: string = apiBaseFromEnv()
): Promise<NativeSpaceAuthResult> {
  return nativeSpacePost(`/auth/spaces/${mode}/`, { email, password }, apiBase);
}

/**
 * Read the live one-time code for an address from the stack redis,
 * standing in for the emailed code the console backend prints. Resolves
 * null when no code is planted (never generated, consumed, or expired).
 */
export async function spaceMagicCode(email: string): Promise<string | null> {
  const container = process.env["PARITY_REDIS_CONTAINER"] ?? "parity19-redis";
  try {
    const { stdout } = await execFileAsync("docker", ["exec", "-i", container, "redis-cli", "GET", `magic_${email}`], {
      timeout: 60_000,
    });
    const raw = stdout.trim();
    if (raw === "") return null;
    const payload = JSON.parse(raw) as { token?: unknown };
    return typeof payload.token === "string" ? payload.token : null;
  } catch {
    return null;
  }
}

/** Drop the live one-time code for an address (expiry stand-in). */
export async function spaceDropMagicCode(email: string): Promise<void> {
  const container = process.env["PARITY_REDIS_CONTAINER"] ?? "parity19-redis";
  await execFileAsync("docker", ["exec", "-i", container, "redis-cli", "DEL", `magic_${email}`], {
    timeout: 60_000,
  });
}

// --- Desktop CLI device-grant endpoints (NEWFRONT-210, DESK-021). Appended;
// --- existing helpers above are untouched per the shared harness contract.
// --- The desktop CLI setup automates both halves of the web /auth/device/
// --- page (overlay pidash-cli.ts); the oracle drives the human-typed page
// --- plus these endpoints directly.

/** Device/user code pair plus the RFC 8628 polling facts. */
export interface ServerDeskShellDeviceGrant {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

/** Anonymous start of a device grant; resolves with the code pair. */
export async function serverDeskShellDeviceStart(
  apiBase: string = apiBaseFromEnv()
): Promise<ServerDeskShellDeviceGrant> {
  const res = await fetchTolerant(`${apiBase}/api/v1/auth/device/start/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({}),
  });
  if (!res.ok) throw new Error(`[parity] device start failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as {
    device_code?: unknown;
    user_code?: unknown;
    verification_uri?: unknown;
    expires_in?: unknown;
    interval?: unknown;
  };
  if (
    typeof payload.device_code !== "string" ||
    typeof payload.user_code !== "string" ||
    typeof payload.verification_uri !== "string" ||
    typeof payload.expires_in !== "number" ||
    typeof payload.interval !== "number"
  ) {
    throw new Error("[parity] device start response carried no code pair.");
  }
  return {
    deviceCode: payload.device_code,
    userCode: payload.user_code,
    verificationUri: payload.verification_uri,
    expiresIn: payload.expires_in,
    interval: payload.interval,
  };
}

/** Session-authenticated approval of a pending grant by its user code. */
export async function serverDeskShellDeviceApprove(
  userCode: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ ok: boolean; userEmail: string; workspaceSlug: string | null }> {
  const res = await authedApi(
    "/v1/auth/device/approve/",
    sessionCookie,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ user_code: userCode }),
    },
    apiBase
  );
  if (!res.ok) throw new Error(`[parity] device approve failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { ok?: unknown; user_email?: unknown; workspace_slug?: unknown };
  if (payload.ok !== true || typeof payload.user_email !== "string") {
    throw new Error("[parity] device approve response carried no approval.");
  }
  return {
    ok: true,
    userEmail: payload.user_email,
    workspaceSlug: typeof payload.workspace_slug === "string" ? payload.workspace_slug : null,
  };
}

/** Raw status of an approval attempt; null session sends no cookie. */
export async function serverDeskShellDeviceApproveStatus(
  userCode: string,
  sessionCookie: string | null,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (sessionCookie !== null) headers["cookie"] = sessionCookie;
  const res = await fetchTolerant(`${apiBase}/api/v1/auth/device/approve/`, {
    method: "POST",
    headers,
    body: JSON.stringify({ user_code: userCode }),
  });
  return res.status;
}

/** Anonymous exchange of an approved device code for a CLI token. */
export async function serverDeskShellDeviceToken(
  deviceCode: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ accessToken: string; tokenType: string; userEmail: string | null; workspaceSlug: string | null }> {
  const res = await fetchTolerant(`${apiBase}/api/v1/auth/device/token/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ device_code: deviceCode }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(`[parity] device token failed with HTTP ${res.status}: ${detail.slice(0, 160)}.`);
  }
  const payload = (await res.json()) as {
    access_token?: unknown;
    token_type?: unknown;
    user_email?: unknown;
    workspace_slug?: unknown;
  };
  if (typeof payload.access_token !== "string" || payload.access_token === "") {
    throw new Error("[parity] device token response carried no token.");
  }
  return {
    accessToken: payload.access_token,
    tokenType: typeof payload.token_type === "string" ? payload.token_type : "",
    userEmail: typeof payload.user_email === "string" ? payload.user_email : null,
    workspaceSlug: typeof payload.workspace_slug === "string" ? payload.workspace_slug : null,
  };
}

/** Raw status plus error code of a token poll (pending grants answer 400). */
export async function serverDeskShellDeviceTokenError(
  deviceCode: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; error: string }> {
  const res = await fetchTolerant(`${apiBase}/api/v1/auth/device/token/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ device_code: deviceCode }),
  });
  const payload = (await res.json()) as { error?: unknown };
  return { status: res.status, error: typeof payload.error === "string" ? payload.error : "" };
}

// --- Analytics overview/work-items chart reads (NEWFRONT-376, ANL-005..009). ---
// Appended; existing helpers above are untouched per the shared contract.

/**
 * Raw charts read for one analytics graph, optionally narrowed to a project
 * subset (ANL-006 radar, ANL-009 created-vs-resolved). Mirrors the query the
 * chart sections send: `type` always, `project_ids` only when the picker
 * holds a non-empty selection.
 */
export async function analyticsAdvanceChartsRaw(
  session: AuthedSession,
  slug: string,
  type: "projects" | "work-items",
  projectIds: string[] = []
): Promise<AnalyticsRawRead> {
  const params = new URLSearchParams({ type });
  if (projectIds.length > 0) params.set("project_ids", projectIds.join(","));
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${slug}/advance-analytics-charts?${params.toString()}`
  );
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw project-stats read backing the active-projects list (ANL-007). Mirrors
 * the list's query, including the `fields` names it asks for (the server
 * answers unknown names with its default full field set).
 */
export async function analyticsProjectStatsRaw(
  session: AuthedSession,
  slug: string,
  fields: string[] = ["total_work_items", "total_completed_work_items"]
): Promise<AnalyticsRawRead> {
  const params = new URLSearchParams({ fields: fields.join(",") });
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/project-stats/?${params.toString()}`);
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

// --- Space engagement server reads (NEWFRONT-451, SPC-047–057). Appended;
// --- existing helpers above are untouched per the shared harness contract.
// --- Reads ride out throttle windows (fetchTolerant); mutations throw HTTP
// --- statuses so withThrottleRetry waits out a saturated window instead of
// --- failing the oracle. Votes and reactions assert against the embedded
// --- detail items: the public votes/reactions list endpoints resolve the
// --- board by the wrong key and always answer [], so they cannot prove
// --- anything (observed live; noted in the area spec).

/** API container for shell snippets; namespaced stacks derive from PARITY_NS. */
function serverSpaceApiContainer(): string {
  const override = process.env["PARITY_API_CONTAINER"];
  if (override !== undefined && override.trim() !== "") return override.trim();
  const ns = process.env["PARITY_NS"];
  if (ns !== undefined && /^\d+$/.test(ns.trim())) return `parity${ns.trim()}-api`;
  return "parity19-api";
}

/** Run a Django shell snippet inside this run's stack api container. */
export async function serverSpaceShell(python: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", serverSpaceApiContainer(), "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/** Board engagement flags plus the published views, as the publish read returns them. */
export interface SpaceBoardFlags {
  comments: boolean;
  reactions: boolean;
  votes: boolean;
  views: Record<string, boolean>;
}

/** Read the seeded board's engagement flags through the publish endpoint. */
export async function serverSpaceBoardFlags(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceBoardFlags> {
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/project-deploy-boards/`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] deploy-board read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as {
    is_comments_enabled?: unknown;
    is_reactions_enabled?: unknown;
    is_votes_enabled?: unknown;
    view_props?: unknown;
  };
  return {
    comments: payload.is_comments_enabled === true,
    reactions: payload.is_reactions_enabled === true,
    votes: payload.is_votes_enabled === true,
    views: (payload.view_props ?? {}) as Record<string, boolean>,
  };
}

/** Resolve a public issue id by its rendered name. */
export async function serverSpaceIssueIdByName(
  anchor: string,
  issueName: string,
  sessionCookie: string = "",
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  const res = await fetchTolerant(`${apiBase}/api/public/anchor/${anchor}/issues/`, {
    headers: sessionCookie === "" ? {} : { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] public issues read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { results?: { id?: unknown; name?: unknown }[] };
  const found = (payload.results ?? []).find((row) => row.name === issueName);
  if (typeof found?.id !== "string" || found.id === "")
    throw new Error(`[parity] no public issue named ${JSON.stringify(issueName)}.`);
  return found.id;
}

/** One embedded vote with its voter, as the issue detail carries it. */
export interface SpaceServerVote {
  vote: number;
  actorId: string;
  displayName: string;
}

/** One embedded reaction with its reactor, as the issue detail carries it. */
export interface SpaceServerReaction {
  reaction: string;
  actorId: string;
  displayName: string;
}

/** Votes plus reactions embedded in the public issue detail (the lists lie; the detail does not). */
export async function serverSpaceIssueEngagement(
  anchor: string,
  issueId: string,
  sessionCookie: string = "",
  apiBase: string = apiBaseFromEnv()
): Promise<{ votes: SpaceServerVote[]; reactions: SpaceServerReaction[] }> {
  const res = await fetchTolerant(`${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/`, {
    headers: sessionCookie === "" ? {} : { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] public issue detail read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as {
    vote_items?: { vote?: unknown; actor_details?: { id?: unknown; display_name?: unknown } }[];
    reaction_items?: { reaction?: unknown; actor_details?: { id?: unknown; display_name?: unknown } }[];
  };
  return {
    votes: (payload.vote_items ?? []).map((row) => ({
      vote: typeof row.vote === "number" ? row.vote : 0,
      actorId: typeof row.actor_details?.id === "string" ? row.actor_details.id : "",
      displayName: typeof row.actor_details?.display_name === "string" ? row.actor_details.display_name : "",
    })),
    reactions: (payload.reaction_items ?? []).map((row) => ({
      reaction: typeof row.reaction === "string" ? row.reaction : "",
      actorId: typeof row.actor_details?.id === "string" ? row.actor_details.id : "",
      displayName: typeof row.actor_details?.display_name === "string" ? row.actor_details.display_name : "",
    })),
  };
}

/** Cast or replace the caller's vote through the public votes endpoint. */
export async function serverSpaceVote(
  anchor: string,
  issueId: string,
  vote: 1 | -1,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space vote", async () => {
    const res = await mutateJSON(
      "POST",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/votes/`,
      sessionCookie,
      { vote }
    );
    if (!res.ok) throw new Error(`[parity] public vote failed with HTTP ${res.status}.`);
  });
}

/** Remove the caller's vote through the public votes endpoint. */
export async function serverSpaceDeleteVote(
  anchor: string,
  issueId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space vote delete", async () => {
    const res = await mutateJSON(
      "DELETE",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/votes/`,
      sessionCookie
    );
    if (!res.ok && res.status !== 404) throw new Error(`[parity] public vote delete failed with HTTP ${res.status}.`);
  });
}

/** Add an issue emoji reaction through the public endpoint (reaction is decimal code points). */
export async function serverSpaceReact(
  anchor: string,
  issueId: string,
  reaction: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space react", async () => {
    const res = await mutateJSON(
      "POST",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/reactions/`,
      sessionCookie,
      { reaction }
    );
    if (!res.ok) throw new Error(`[parity] public react failed with HTTP ${res.status}.`);
  });
}

/** Remove the caller's issue emoji reaction through the public endpoint. */
export async function serverSpaceDeleteReaction(
  anchor: string,
  issueId: string,
  reaction: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space react delete", async () => {
    const res = await mutateJSON(
      "DELETE",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/reactions/${reaction}/`,
      sessionCookie
    );
    if (!res.ok && res.status !== 404) throw new Error(`[parity] public react delete failed with HTTP ${res.status}.`);
  });
}

/** One public comment row with the fields the overlay renders. */
export interface SpaceServerComment {
  id: string;
  html: string;
  stripped: string;
  labels: string[];
  speakerType: string;
  speakerLabel: string;
  actorId: string;
  displayName: string;
  firstName: string;
  isBot: boolean;
  avatarUrl: string;
  createdAt: string;
  reactions: { reaction: string; actorId: string; displayName: string }[];
}

function spaceServerCommentOf(row: {
  id?: unknown;
  comment_html?: unknown;
  comment_stripped?: unknown;
  labels?: unknown;
  speaker_type?: unknown;
  speaker_label?: unknown;
  actor_detail?: {
    id?: unknown;
    display_name?: unknown;
    first_name?: unknown;
    is_bot?: unknown;
    avatar_url?: unknown;
    avatar?: unknown;
  };
  created_at?: unknown;
  comment_reactions?: {
    reaction?: unknown;
    actor?: unknown;
    display_name?: unknown;
    actor_detail?: { id?: unknown; display_name?: unknown };
  }[];
}): SpaceServerComment {
  return {
    id: typeof row.id === "string" ? row.id : "",
    html: typeof row.comment_html === "string" ? row.comment_html : "",
    stripped: typeof row.comment_stripped === "string" ? row.comment_stripped : "",
    labels: Array.isArray(row.labels) ? row.labels.filter((label): label is string => typeof label === "string") : [],
    speakerType: typeof row.speaker_type === "string" ? row.speaker_type : "",
    speakerLabel: typeof row.speaker_label === "string" ? row.speaker_label : "",
    actorId: typeof row.actor_detail?.id === "string" ? row.actor_detail.id : "",
    displayName: typeof row.actor_detail?.display_name === "string" ? row.actor_detail.display_name : "",
    firstName: typeof row.actor_detail?.first_name === "string" ? row.actor_detail.first_name : "",
    isBot: row.actor_detail?.is_bot === true,
    avatarUrl:
      typeof row.actor_detail?.avatar_url === "string" && row.actor_detail.avatar_url !== ""
        ? row.actor_detail.avatar_url
        : typeof row.actor_detail?.avatar === "string"
          ? row.actor_detail.avatar
          : "",
    createdAt: typeof row.created_at === "string" ? row.created_at : "",
    // Comment reactions serialize their actor flat (actor/display_name),
    // unlike the nested actor_detail on issue reactions (NEWFRONT-466).
    reactions: (row.comment_reactions ?? []).map((reaction) => ({
      reaction: typeof reaction.reaction === "string" ? reaction.reaction : "",
      actorId:
        typeof reaction.actor_detail?.id === "string"
          ? reaction.actor_detail.id
          : typeof reaction.actor === "string"
            ? reaction.actor
            : "",
      displayName:
        typeof reaction.actor_detail?.display_name === "string"
          ? reaction.actor_detail.display_name
          : typeof reaction.display_name === "string"
            ? reaction.display_name
            : "",
    })),
  };
}

/** List an issue's public comments (anonymous read; the thread is public). */
export async function serverSpaceComments(
  anchor: string,
  issueId: string,
  sessionCookie: string = "",
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceServerComment[]> {
  const res = await fetchTolerant(`${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/comments/`, {
    headers: sessionCookie === "" ? {} : { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] public comments read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows = (Array.isArray(payload) ? payload : []) as Parameters<typeof spaceServerCommentOf>[0][];
  return rows.map(spaceServerCommentOf);
}

/** Post a public comment; extra carries fixture fields (labels, speaker_*). */
export async function serverSpacePostComment(
  anchor: string,
  issueId: string,
  html: string,
  stripped: string,
  sessionCookie: string,
  extra: Record<string, unknown> = {},
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceServerComment> {
  return withThrottleRetry("space comment post", async () => {
    const res = await mutateJSON(
      "POST",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/comments/`,
      sessionCookie,
      { comment_html: html, comment_stripped: stripped, ...extra }
    );
    if (!res.ok) throw new Error(`[parity] public comment post failed with HTTP ${res.status}.`);
    return spaceServerCommentOf((await res.json()) as Parameters<typeof spaceServerCommentOf>[0]);
  });
}

/** Delete a public comment the session owns. */
export async function serverSpaceDeleteComment(
  anchor: string,
  issueId: string,
  commentId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space comment delete", async () => {
    const res = await mutateJSON(
      "DELETE",
      `${apiBase}/api/public/anchor/${anchor}/issues/${issueId}/comments/${commentId}/`,
      sessionCookie
    );
    if (!res.ok && res.status !== 404)
      throw new Error(`[parity] public comment delete failed with HTTP ${res.status}.`);
  });
}

/** Add a comment emoji reaction through the public endpoint. */
export async function serverSpaceCommentReact(
  anchor: string,
  commentId: string,
  reaction: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space comment react", async () => {
    const res = await mutateJSON(
      "POST",
      `${apiBase}/api/public/anchor/${anchor}/comments/${commentId}/reactions/`,
      sessionCookie,
      { reaction }
    );
    if (!res.ok) throw new Error(`[parity] public comment react failed with HTTP ${res.status}.`);
  });
}

/** Remove the caller's comment emoji reaction through the public endpoint. */
export async function serverSpaceDeleteCommentReaction(
  anchor: string,
  commentId: string,
  reaction: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  await withThrottleRetry("space comment react delete", async () => {
    const res = await mutateJSON(
      "DELETE",
      `${apiBase}/api/public/anchor/${anchor}/comments/${commentId}/reactions/${reaction}/`,
      sessionCookie
    );
    if (!res.ok && res.status !== 404)
      throw new Error(`[parity] public comment react delete failed with HTTP ${res.status}.`);
  });
}

/** One public board member, as the mention source lists them. */
export interface SpaceServerMember {
  id: string;
  displayName: string;
}

/** List the board's public members (the mention autocomplete source). */
export async function serverSpaceMembers(
  anchor: string,
  sessionCookie: string = "",
  apiBase: string = apiBaseFromEnv()
): Promise<SpaceServerMember[]> {
  const res = await fetchTolerant(`${apiBase}/api/public/anchor/${anchor}/members/`, {
    headers: sessionCookie === "" ? {} : { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] public members read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows = (Array.isArray(payload) ? payload : []) as {
    member__id?: unknown;
    member__display_name?: unknown;
    id?: unknown;
    display_name?: unknown;
  }[];
  return rows.map((row) => ({
    id: typeof row.member__id === "string" ? row.member__id : typeof row.id === "string" ? row.id : "",
    displayName:
      typeof row.member__display_name === "string"
        ? row.member__display_name
        : typeof row.display_name === "string"
          ? row.display_name
          : "",
  }));
}

/**
 * Converge a distinct-names bot identity for the bot-marking proof (the
 * seeded bot renders identically down both branches, so it cannot prove
 * the marking). Convergent across reseeds and reruns; no cleanup needed.
 */
export async function serverSpacePlantBot(
  email: string = "parity-space-bot@example.com",
  password: string = "Parity-Seed-2"
): Promise<{ email: string; password: string; id: string; displayName: string; firstName: string }> {
  const out = await serverSpaceShell(
    `from django.contrib.auth.hashers import make_password\n` +
      `from pi_dash.db.models import User\n` +
      `user, _ = User.objects.get_or_create(email=${JSON.stringify(email)}, defaults={"username": "parity_space_bot"})\n` +
      `user.password = make_password(${JSON.stringify(password)})\n` +
      `user.display_name = "Zeddy Spacebotte"\n` +
      `user.first_name = "Zed"\n` +
      `user.last_name = "Spacebotte"\n` +
      `user.is_active = True\n` +
      `user.is_email_verified = True\n` +
      `user.is_bot = True\n` +
      `user.save()\n` +
      `print("PARITY_SPACE_BOT:" + str(user.id))\n`
  );
  const id = /^PARITY_SPACE_BOT:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (id === "") throw new Error("[parity] bot plant produced no id.");
  return { email, password, id, displayName: "Zeddy Spacebotte", firstName: "Zed" };
}

// --- Cloud oracle session + quota helpers (NEWFRONT-437, CLOUD-010).
// --- Appended; existing helpers above are untouched per the shared harness
// --- contract. The cloud oracle has no password sign-in: scenarios enter
// --- through the dev-login endpoint (which anchors on email, so the seed
// --- address returns the seed user) and carry the JWT cookies it sets.

/** Authenticated cloud caller: cookie header plus the user id it belongs to. */
export interface CloudSession {
  /** `Cookie:` header value for authenticated API calls as this user. */
  cookie: string;
  /** UUID string of the signed-in user. */
  userId: string;
}

/**
 * Sign in to the cloud oracle through the dev-login bypass (no provider
 * round trip, no AIREPUBLIC_* secrets). Resolves with the session cookies
 * as one header value plus the user id.
 */
export async function cloudDevLogin(
  email: string,
  displayName?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudSession> {
  const res = await fetchTolerant(`${apiBase}/api/auth/dev-login/`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(displayName === undefined ? { email } : { email, display_name: displayName }),
  });
  if (!res.ok) throw new Error(`[parity] cloud dev-login failed with HTTP ${res.status} for ${email}.`);
  const header = cookieHeader(setCookieHeaders(res));
  if (!header.includes("pidash_access="))
    throw new Error("[parity] cloud dev-login response carried no session cookie.");
  const payload = (await res.json()) as { user?: { id?: unknown } };
  const userId = payload.user?.id;
  if (typeof userId !== "string" || userId === "")
    throw new Error("[parity] cloud dev-login response carried no user id.");
  return { cookie: header, userId };
}

/** Browser cookies for a cloud session, shaped for `context.addCookies`. */
export function cloudBrowserCookies(sessionCookie: string): BrowserCookie[] {
  const jar = cookieJar(sessionCookie.split(";").map((pair) => pair.trim()));
  const access = jar.get("pidash_access");
  if (access === undefined) throw new Error("[parity] cloud session cookie carried no pidash_access.");
  const domain = oracleHostFromEnv();
  const cookies: BrowserCookie[] = [
    { name: "pidash_access", value: access, domain, path: "/", httpOnly: true, sameSite: "Lax" },
  ];
  const refresh = jar.get("pidash_refresh");
  if (refresh !== undefined) {
    // The server scopes the refresh cookie to the refresh endpoint and
    // expires that same path on sign-out; a path-/ twin would survive the
    // sign-out and resurrect the session (NEWFRONT-438).
    cookies.push({
      name: "pidash_refresh",
      value: refresh,
      domain,
      path: "/api/auth/refresh/",
      httpOnly: true,
      sameSite: "Lax",
    });
  }
  return cookies;
}

/** Raw issue-create answer: the HTTP status plus the decoded JSON payload. */
export interface CloudCreateAnswer {
  status: number;
  payload: Record<string, unknown>;
}

/**
 * Create one issue without throwing: resolves with the status plus payload
 * so quota scenarios can assert the 402 refusal shape itself. The REST
 * APIs are CSRF-exempt, so the session cookie header is sufficient.
 */
export async function cloudCreateIssueRaw(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  name: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudCreateAnswer> {
  const res = await authed(
    workspaceSlug,
    `/projects/${projectId}/issues/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name }) },
    apiBase
  );
  let payload: Record<string, unknown> = {};
  try {
    payload = (await res.json()) as Record<string, unknown>;
  } catch {
    payload = {};
  }
  return { status: res.status, payload };
}

// --- Analytics custom insights / tables / scoped dialog reads (NEWFRONT-377, ANL-010..017). ---
// Appended; existing helpers above are untouched per the shared contract.
// Mirrors the queries the custom-insights chart, the state-breakdown table,
// and the project-scoped dialog send: the custom graph carries its axis
// params on the charts read, the breakdown reads the stats endpoint, and
// the dialog reads the project-scoped advance-analytics variants.

/**
 * Raw custom-graph charts read (ANL-010..012). Mirrors the query the
 * custom-insights section sends: `type=custom-work-items` always, the axis
 * params always (group_by omitted when no second split is chosen), and
 * `project_ids` only when the picker holds a non-empty selection.
 */
export async function analyticsAdvanceCustomChartsRaw(
  session: AuthedSession,
  slug: string,
  params: { x_axis: string; y_axis: string; group_by?: string },
  projectIds: string[] = []
): Promise<AnalyticsRawRead> {
  const query = new URLSearchParams({ type: "custom-work-items", x_axis: params.x_axis, y_axis: params.y_axis });
  if (params.group_by !== undefined && params.group_by !== "") query.set("group_by", params.group_by);
  if (projectIds.length > 0) query.set("project_ids", projectIds.join(","));
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${slug}/advance-analytics-charts?${query.toString()}`
  );
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw state-breakdown stats read (ANL-013). Mirrors the insight table's
 * query: `type=work-items` always, `project_ids` only under a picker
 * selection.
 */
export async function analyticsAdvanceStatsRaw(
  session: AuthedSession,
  slug: string,
  projectIds: string[] = []
): Promise<AnalyticsRawRead> {
  const query = new URLSearchParams({ type: "work-items" });
  if (projectIds.length > 0) query.set("project_ids", projectIds.join(","));
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/advance-analytics-stats?${query.toString()}`);
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw project-scoped headline-totals read (ANL-014 dialog). Mirrors the
 * dialog's peek-view totals query: the project path plus `tab` and the
 * scoping params (`project_ids` always, `cycle_id`/`module_id` when the
 * dialog opened from those surfaces).
 */
export async function analyticsScopedTotalsRaw(
  session: AuthedSession,
  slug: string,
  projectId: string,
  extra: Record<string, string> = {}
): Promise<AnalyticsRawRead> {
  const query = new URLSearchParams({ tab: "work-items", project_ids: projectId, ...extra });
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${slug}/projects/${projectId}/advance-analytics?${query.toString()}`
  );
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw project-scoped charts read (ANL-014 dialog). Same scoping shape as
 * the totals variant, with the graph `type` in place of `tab`; `extra`
 * carries axis params for the custom graph or cycle/module scoping.
 */
export async function analyticsScopedChartsRaw(
  session: AuthedSession,
  slug: string,
  projectId: string,
  type: string,
  extra: Record<string, string> = {}
): Promise<AnalyticsRawRead> {
  const query = new URLSearchParams({ type, project_ids: projectId, ...extra });
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${slug}/projects/${projectId}/advance-analytics-charts?${query.toString()}`
  );
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/**
 * Raw project-scoped stats read (ANL-014/015 dialog table). Same scoping
 * shape as the totals variant with the stats `type`.
 */
export async function analyticsScopedStatsRaw(
  session: AuthedSession,
  slug: string,
  projectId: string,
  extra: Record<string, string> = {}
): Promise<AnalyticsRawRead> {
  const query = new URLSearchParams({ type: "work-items", project_ids: projectId, ...extra });
  const res = await sessionFetch(
    session,
    "GET",
    `/api/workspaces/${slug}/projects/${projectId}/advance-analytics-stats?${query.toString()}`
  );
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    payload = null;
  }
  return { status: res.status, payload };
}

/** One project state with the display color the state-axis bars reuse (ANL-011). */
export interface AnalyticsStateColor {
  id: string;
  name: string;
  color: string;
}

/**
 * Project states with display colors (ANL-011 state-axis fills). The shared
 * states helper drops the color field, so the oracle reads it directly.
 */
export async function analyticsProjectStateColors(
  session: AuthedSession,
  slug: string,
  projectId: string
): Promise<AnalyticsStateColor[]> {
  const res = await sessionFetch(session, "GET", `/api/workspaces/${slug}/projects/${projectId}/states/`);
  if (!res.ok) throw new Error(`[parity] state color read failed with HTTP ${res.status}.`);
  const rows: unknown = await res.json();
  const list: unknown[] = Array.isArray(rows) ? rows : [];
  return list.map((row) => {
    const record = row as { id?: unknown; name?: unknown; color?: unknown };
    if (typeof record.id !== "string" || typeof record.name !== "string" || typeof record.color !== "string") {
      throw new Error("[parity] state row carried no id/name/color.");
    }
    return { id: record.id, name: record.name, color: record.color };
  });
}

// --- Cloud login/SSO/session helpers (NEWFRONT-438, CLOUD-011..020).
// --- Appended; existing helpers above are untouched per the shared harness
// --- contract. Real airepublic SSO is unavailable on runners, so these prove
// --- the handoff up to the provider boundary (redirect target + parameters)
// --- and the session endpoints' own contracts.

/** A redirect answer without following it: status plus the Location header. */
export interface CloudRedirectAnswer {
  status: number;
  location: string;
}

/**
 * Ask the cloud sign-in endpoint where it would send the browser, without
 * following the redirect. `params` are the query pairs the app would send
 * (return_to, return_origin, desktop).
 */
export async function cloudSignInRedirect(
  params: Record<string, string>,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudRedirectAnswer> {
  const query = new URLSearchParams(params).toString();
  const res = await fetchTolerant(`${apiBase}/api/auth/sign-in/?${query}`, { redirect: "manual" });
  return { status: res.status, location: res.headers.get("location") ?? "" };
}

/**
 * Ask the desktop exchange endpoint where it would send the webview,
 * without following the redirect. Serves the negative shapes (missing or
 * unknown code/state); the positive exchange needs the real provider.
 */
export async function cloudDesktopExchangeStatus(
  params: Record<string, string>,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudRedirectAnswer> {
  const query = new URLSearchParams(params).toString();
  const res = await fetchTolerant(`${apiBase}/api/auth/desktop-exchange/?${query}`, { redirect: "manual" });
  return { status: res.status, location: res.headers.get("location") ?? "" };
}

/** A refresh answer: status plus the raw Set-Cookie lines. */
export interface CloudRefreshAnswer {
  status: number;
  setCookies: string[];
}

/**
 * Rotate a cloud session through the refresh endpoint with the given
 * cookie header. A live refresh cookie answers 200 and re-sets the pair;
 * anything else answers 401.
 */
export async function cloudRefreshStatus(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudRefreshAnswer> {
  const res = await fetchTolerant(`${apiBase}/api/auth/refresh/`, {
    method: "POST",
    headers: { cookie: sessionCookie },
  });
  return { status: res.status, setCookies: setCookieHeaders(res) };
}

/** A sign-out answer: status plus the raw Set-Cookie lines and payload. */
export interface CloudSignOutAnswer {
  status: number;
  setCookies: string[];
  payload: Record<string, unknown>;
}

/**
 * Sign a cloud session out the way the app does: fetch a CSRF token (which
 * also sets the csrf cookie), then post with both cookie and header. The
 * session cookies come back expired; the payload names the upstream
 * provider logout URL the web app deliberately never visits.
 */
export async function cloudSignOutServer(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudSignOutAnswer> {
  const csrfRes = await fetchTolerant(`${apiBase}/api/auth/get-csrf-token/`, {
    headers: { cookie: sessionCookie },
  });
  if (!csrfRes.ok) throw new Error(`[parity] cloud csrf read failed with HTTP ${csrfRes.status}.`);
  const csrfPayload = (await csrfRes.json()) as { csrf_token?: unknown };
  if (typeof csrfPayload.csrf_token !== "string" || csrfPayload.csrf_token === "") {
    throw new Error("[parity] cloud csrf read carried no token.");
  }
  const jar = cookieJar(
    sessionCookie.split(";").map((pair) => pair.trim()),
    setCookieHeaders(csrfRes)
  );
  const merged = [...jar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
  const res = await fetchTolerant(`${apiBase}/api/auth/sign-out/`, {
    method: "POST",
    headers: {
      cookie: merged,
      "content-type": "application/json",
      "X-CSRFTOKEN": csrfPayload.csrf_token,
    },
    body: "{}",
  });
  const payload = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  return { status: res.status, setCookies: setCookieHeaders(res), payload };
}

// --- Cloud Apps catalog server reads (NEWFRONT-439, CLOUD-021..031). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. Raw (non-throwing) reads of the openhub proxy endpoints plus
// cloud member provisioning, so Apps scenarios assert live server state
// (what Django answers itself, with no gateway behind scratch) alongside
// what the driver sees in the browser.

/** Raw openhub answer: the HTTP status plus the decoded JSON payload. */
export interface CloudOpenhubAnswer {
  status: number;
  payload: Record<string, unknown>;
}

async function cloudOpenhubRaw(
  path: string,
  sessionCookie: string,
  init: RequestInit = {},
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  const res = await fetchTolerant(`${apiBase}${path}`, {
    ...init,
    headers: { ...(init.headers ?? {}), cookie: sessionCookie },
  });
  let payload: Record<string, unknown> = {};
  try {
    payload = (await res.json()) as Record<string, unknown>;
  } catch {
    payload = {};
  }
  return { status: res.status, payload };
}

/** Live openhub status probe (200 with connected:false behind scratch). */
export async function cloudOpenhubStatusRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(`/api/openhub/status/`, sessionCookie, {}, apiBase);
}

/** Live marketplace read (taxonomy failure behind scratch, no gateway). */
export async function cloudMarketplaceRaw(
  sessionCookie: string,
  query: string = "limit=24",
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(`/api/openhub/apps/marketplace/?${query}`, sessionCookie, {}, apiBase);
}

/** Live install attempt (taxonomy failure behind scratch, no gateway). */
export async function cloudInstallRaw(
  sessionCookie: string,
  appId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(
    `/api/openhub/apps/${encodeURIComponent(appId)}/install/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({}) },
    apiBase
  );
}

/** Live per-app auth-status read (taxonomy failure behind scratch). */
export async function cloudAuthStatusRaw(
  sessionCookie: string,
  appId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(`/api/openhub/apps/${encodeURIComponent(appId)}/auth/status/`, sessionCookie, {}, apiBase);
}

/** Live OAuth begin handoff without following the redirect. */
export async function cloudOAuthBeginRaw(
  sessionCookie: string,
  appId: string,
  returnPath: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; location: string }> {
  const res = await fetchTolerant(
    `${apiBase}/api/openhub/apps/${encodeURIComponent(appId)}/auth/oauth/begin/?return_path=${encodeURIComponent(returnPath)}`,
    { headers: { cookie: sessionCookie }, redirect: "manual" }
  );
  return { status: res.status, location: res.headers.get("location") ?? "" };
}

/** Live credential submit (validates fields before any gateway call). */
export async function cloudApiKeyRaw(
  sessionCookie: string,
  appId: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(
    `/api/openhub/apps/${encodeURIComponent(appId)}/auth/api-key/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
}

/** Live icon-proxy read (validates shape before fetching). */
export async function cloudIconRaw(
  sessionCookie: string,
  iconUrl: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; contentType: string }> {
  const res = await fetchTolerant(`${apiBase}/api/openhub/apps/icon/?url=${encodeURIComponent(iconUrl)}`, {
    headers: { cookie: sessionCookie },
  });
  return { status: res.status, contentType: res.headers.get("content-type") ?? "" };
}

/** Live profile-settings read (the agent-tools flag lives here). */
export async function cloudProfileRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(`/api/users/me/profile/`, sessionCookie, {}, apiBase);
}

/** Live profile-settings write (namespaced merge, per-setting keys). */
export async function cloudProfilePatchRaw(
  sessionCookie: string,
  body: Record<string, unknown>,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudOpenhubAnswer> {
  return cloudOpenhubRaw(
    `/api/users/me/profile/`,
    sessionCookie,
    { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
}

/**
 * Mark a cloud user onboarded through the app's own endpoint. The cloud
 * REST APIs are CSRF-exempt and the call is idempotent, so provisioning
 * converges by calling it unconditionally (no apiShell: stack helpers
 * target the OSS api container, not api-cloud).
 */
export async function cloudCompleteOnboarding(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetchTolerant(`${apiBase}/api/users/me/onboard/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", cookie: sessionCookie },
    body: JSON.stringify({ is_onboarded: true }),
  });
  if (!res.ok) throw new Error(`[parity] cloud onboard PATCH failed with HTTP ${res.status}.`);
}

/**
 * Provision a workspace member on the cloud oracle: dev-login anchors the
 * user by email (idempotent), then invite + join unless already a member,
 * then onboard. Convergent across retries: every step is a no-op when its
 * outcome already holds. The caller keeps the user (user rows belong to
 * the scratch stack and vanish with it).
 */
export async function cloudProvisionWorkspaceMember(
  workspaceSlug: string,
  email: string,
  role: number,
  ownerCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudSession> {
  const session = await cloudDevLogin(email, undefined, apiBase);
  const members = await serverWorkspaceMembers(workspaceSlug, ownerCookie, apiBase);
  if (!members.some((row) => row.userId === session.userId)) {
    // A retry may find the invite already pending (join never ran): the
    // re-invite then fails, which is fine — the pending row below is the
    // convergence point, not the POST.
    await inviteWorkspaceMember(workspaceSlug, ownerCookie, email, role, apiBase).catch(() => undefined);
    const invites = await listWorkspaceInvites(workspaceSlug, ownerCookie, apiBase);
    const invite = invites.find((row) => row.email === email);
    if (invite === undefined) throw new Error(`[parity] cloud invite for ${email} left no pending row.`);
    await joinWorkspace(workspaceSlug, invite.id, invite.token, session.cookie, apiBase);
  }
  await cloudCompleteOnboarding(session.cookie, apiBase);
  return session;
}

// --- Cloud profile tabs + misc (NEWFRONT-440, CLOUD-032..040). ---
// Appended; existing helpers above are untouched per the shared harness
// contract. Raw reads/writes against the cloud assistant, billing,
// instance, and agent endpoints (the session cookie header is sufficient;
// the REST APIs are CSRF-exempt), plus Django-shell fixtures for states
// no API can plant: an account-connected credential (real OAuth is
// unreachable from runners) and a desktop-session token (the scratch IdP
// has no desktop client).

/** Status plus decoded body for a cloud assistant/billing/agent call. */
export interface CloudAssistantAnswer {
  status: number;
  payload: Record<string, unknown>;
}

async function cloudAssistantFetch(
  path: string,
  sessionCookie: string,
  init?: RequestInit,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  const res = await fetchTolerant(`${apiBase}${path}`, {
    ...init,
    headers: { cookie: sessionCookie, ...(init?.headers ?? {}) },
  });
  let payload: Record<string, unknown> = {};
  try {
    payload = (await res.json()) as Record<string, unknown>;
  } catch {
    payload = {};
  }
  return { status: res.status, payload };
}

/** Raw connections-list read (CLOUD-032/033/034). */
export async function cloudAssistantConnectionsRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/users/me/ai-assistant/connections/", sessionCookie, undefined, apiBase);
}

/** Raw self-supplied-key connection create (CLOUD-033). */
export async function cloudAssistantCreateByokRaw(
  sessionCookie: string,
  body: { provider_kind: string; base_url?: string; model_name: string; api_key: string },
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    "/api/users/me/ai-assistant/connections/",
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
}

/** Raw connection select (CLOUD-032). */
export async function cloudAssistantSelectRaw(
  sessionCookie: string,
  connectionId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/users/me/ai-assistant/connections/${connectionId}/select/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
}

/** Raw connection model update (CLOUD-032). */
export async function cloudAssistantPatchModelRaw(
  sessionCookie: string,
  connectionId: string,
  modelName: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/users/me/ai-assistant/connections/${connectionId}/`,
    sessionCookie,
    {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model_name: modelName }),
    },
    apiBase
  );
}

/** Raw connection delete (CLOUD-032). */
export async function cloudAssistantDeleteRaw(
  sessionCookie: string,
  connectionId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/users/me/ai-assistant/connections/${connectionId}/`,
    sessionCookie,
    { method: "DELETE" },
    apiBase
  );
}

/** Raw account-connection disconnect (CLOUD-032). */
export async function cloudAssistantDisconnectRaw(
  sessionCookie: string,
  connectionId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/users/me/ai-assistant/connections/${connectionId}/disconnect/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
}

/** Raw provider-config read (CLOUD-037). */
export async function cloudAssistantConfigRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/users/me/ai-assistant/config/", sessionCookie, undefined, apiBase);
}

/** Raw active-config test (CLOUD-032). */
export async function cloudAssistantTestRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    "/api/users/me/ai-assistant/config/test/",
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
}

/** Raw provider-login start (CLOUD-033). */
export async function cloudOpenaiLoginStartRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    "/api/users/me/ai-assistant/openai-login/",
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
}

/** Raw provider-login status poll (CLOUD-033). */
export async function cloudOpenaiLoginStatusRaw(
  sessionCookie: string,
  state: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/users/me/ai-assistant/openai-login/status/?state=${encodeURIComponent(state)}`,
    sessionCookie,
    undefined,
    apiBase
  );
}

/** Raw billing-plan read (CLOUD-038). */
export async function cloudBillingPlanRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/billing/me/plan/", sessionCookie, undefined, apiBase);
}

/** Raw instance read; the security-tab guard keys off config.is_self_managed (CLOUD-035). */
export async function cloudInstanceRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/instances/", sessionCookie, undefined, apiBase);
}

/** Raw project read for the agent-executor options (CLOUD-040). */
export async function cloudProjectRaw(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    undefined,
    apiBase
  );
}

/** Raw desktop-engine model profile read (CLOUD-040). */
export async function cloudAgentProfileRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/users/me/ai-assistant/agent-profile/", sessionCookie, undefined, apiBase);
}

/** Raw desktop-engine credential mint (CLOUD-040). */
export async function cloudAgentTokenRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch(
    "/api/users/me/ai-assistant/agent-token/",
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
    apiBase
  );
}

/** Raw cloud CSRF-token read (CLOUD-040). */
export async function cloudCsrfRaw(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<CloudAssistantAnswer> {
  return cloudAssistantFetch("/api/auth/get-csrf-token/", sessionCookie, undefined, apiBase);
}

/**
 * Run a Django shell snippet inside this run's cloud api container;
 * resolves with stdout. The container is <project>-api-cloud — derived
 * from the namespace, not from PARITY_API_CONTAINER, which names the OSS
 * api service even on cloud stacks.
 */
export async function cloudShell(python: string): Promise<string> {
  const project = resolveParityEnv(process.env).project;
  const { stdout } = await execFileAsync(
    "docker",
    ["exec", "-i", `${project}-api-cloud`, "python", "manage.py", "shell", "-c", python],
    {
      timeout: 120_000,
    }
  );
  return stdout;
}

/**
 * Mint a desktop-session access token for an existing user (CLOUD-040).
 * The scratch IdP has no desktop OIDC client, so the agent-profile
 * classification is unreachable through any login flow; minting through
 * the same token issuer the views verify keeps the fixture honest.
 */
export async function cloudDesktopAccessToken(email: string): Promise<string> {
  const out = await cloudShell(
    `from pi_dash.db.models import User\n` +
      `from pi_dash_cloud.airepublic import tokens\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `pair = tokens.mint_pair(str(user.id), client="desktop")\n` +
      `print("PARITY_DESKTOP:" + pair.access)\n`
  );
  const token = /^PARITY_DESKTOP:(.+)$/m.exec(out)?.[1]?.trim() ?? "";
  if (token === "") throw new Error(`[parity] desktop token mint produced no token for ${email}.`);
  return token;
}

/**
 * Plant an account-connected credential for an existing user (CLOUD-032).
 * Real provider OAuth is unreachable from runners; the planted row carries
 * a shaped-but-unusable token, which is enough for every connected-state
 * assertion short of a live provider turn (the test endpoint reports the
 * failure inline, which is itself asserted).
 */
export async function cloudPlantCodexCredential(
  email: string,
  opts: { planClientId?: string; modelName?: string } = {}
): Promise<void> {
  const clientId = opts.planClientId ?? "parity-fixture-client";
  const modelName = opts.modelName ?? "gpt-5.6-sol";
  await cloudShell(
    `from pi_dash.db.models import User\n` +
      `from pi_dash_cloud.assistant import services\n` +
      `from pi_dash_cloud.assistant.models import UserCodexCredential\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `# A discovered catalog like a real connected account carries: without\n` +
      `# it the picker falls back to display-only defaults the model-update\n` +
      `# validation then refuses.\n` +
      `catalog = services.codex_model_options(None)\n` +
      `UserCodexCredential.objects.update_or_create(user=user, defaults={` +
      `"access_token_encrypted": b"parity-fixture-token", ` +
      `"oauth_client_id": ${JSON.stringify(clientId)}, ` +
      `"model_name": ${JSON.stringify(modelName)}, "is_active": True, ` +
      `"available_models": catalog})\n` +
      `print("PARITY_CODEX:planted")\n`
  );
}

/** Stamp a connection row verified so the list renders its timestamp (CLOUD-032). */
export async function cloudStampConnectionVerified(connectionId: string): Promise<void> {
  await cloudShell(
    `from django.utils import timezone\n` +
      `from pi_dash_cloud.assistant.models import UserLLMConnection\n` +
      `UserLLMConnection.objects.filter(pk=${JSON.stringify(connectionId)}).update(last_verified_at=timezone.now())\n` +
      `print("PARITY_VERIFIED:stamped")\n`
  );
}

// --- Instance-admin general/email oracle (NEWFRONT-442, ADM-013..022).
// --- Server reads/writes for the general + email settings specs. Appended;
// --- existing helpers above are untouched per the shared harness contract.

/** Instance name plus telemetry flag as the read endpoints report them. */
export interface ServerAdminGeneralSettings {
  instanceName: string;
  telemetryEnabled: boolean;
}

/** Raw instance PATCH round-trip: HTTP status plus the decoded JSON body. */
export interface ServerAdminInstancePatchResult {
  status: number;
  body: unknown;
}

/**
 * CSRF header pair for session-authenticated admin writes. The joined
 * session string can carry the csrftoken twice (the sign-in response
 * rotates the token the pre-login fetch set), so the cookie is rebuilt
 * last-wins per key — exactly what a browser would send — and the
 * header carries the surviving token.
 */
function serverAdminCsrfHeaders(sessionCookie: string): Record<string, string> {
  const pairs = new Map<string, string>();
  for (const part of sessionCookie.split(";")) {
    const trimmed = part.trim();
    if (trimmed === "") continue;
    const cut = trimmed.indexOf("=");
    if (cut <= 0) continue;
    pairs.set(trimmed.slice(0, cut), trimmed);
  }
  const csrf = pairs.get("csrftoken")?.slice("csrftoken=".length) ?? "";
  if (csrf === "") throw new Error("[parity] admin session carries no CSRF token.");
  return { cookie: [...pairs.values()].join("; "), "X-CSRFToken": csrf };
}

/** Raw PATCH /api/instances/ round-trip; never throws on error statuses. */
export async function serverAdminUpdateInstanceRaw(
  sessionCookie: string,
  data: { instance_name?: string; is_telemetry_enabled?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminInstancePatchResult> {
  const res = await fetch(`${apiBase}/api/instances/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify(data),
  });
  return { status: res.status, body: (await res.json().catch(() => null)) as unknown };
}

/** PATCH the instance name and/or telemetry flag; throws unless the write lands. */
export async function serverAdminUpdateInstance(
  sessionCookie: string,
  data: { instanceName?: string; telemetryEnabled?: boolean },
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminGeneralSettings> {
  const out = await serverAdminUpdateInstanceRaw(
    sessionCookie,
    {
      ...(data.instanceName === undefined ? {} : { instance_name: data.instanceName }),
      ...(data.telemetryEnabled === undefined ? {} : { is_telemetry_enabled: data.telemetryEnabled }),
    },
    apiBase
  );
  if (out.status < 200 || out.status >= 300) throw new Error(`[parity] instance PATCH failed with HTTP ${out.status}.`);
  const row = (out.body ?? {}) as { instance_name?: unknown; is_telemetry_enabled?: unknown };
  return {
    instanceName: typeof row.instance_name === "string" ? row.instance_name : "",
    telemetryEnabled: row.is_telemetry_enabled === true,
  };
}

/** Instance configurations as GET reports them (secrets arrive decrypted). */
export async function serverAdminGetConfigurations(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, string>> {
  const res = await fetch(`${apiBase}/api/instances/configurations/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] configurations read failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ key?: unknown; value?: unknown }>;
  const map: Record<string, string> = {};
  for (const row of rows) {
    if (typeof row.key === "string") map[row.key] = typeof row.value === "string" ? row.value : "";
  }
  return map;
}

/**
 * PATCH instance configurations; resolves with the rows the server
 * reports updated. Only keys with an existing row are written (the
 * endpoint skips the rest), and values arrive stripped server-side.
 */
export async function serverAdminUpdateConfigurations(
  sessionCookie: string,
  data: Record<string, string>,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, string>> {
  const res = await fetch(`${apiBase}/api/instances/configurations/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify(data),
  });
  if (!res.ok) throw new Error(`[parity] configurations PATCH failed with HTTP ${res.status}.`);
  const rows = (await res.json()) as Array<{ key?: unknown; value?: unknown }>;
  const map: Record<string, string> = {};
  for (const row of rows) {
    if (typeof row.key === "string") map[row.key] = typeof row.value === "string" ? row.value : "";
  }
  return map;
}

/** Raw test-email round-trip: HTTP status plus the decoded JSON body (or null). */
export interface ServerAdminEmailCheckResult {
  status: number;
  body: unknown;
}

/** POST the test-email check; never throws on error statuses. */
export async function serverAdminEmailCheck(
  sessionCookie: string,
  receiverEmail: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminEmailCheckResult> {
  const res = await fetch(`${apiBase}/api/instances/email-credentials-check/`, {
    method: "POST",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify({ receiver_email: receiverEmail }),
  });
  return { status: res.status, body: (await res.json().catch(() => null)) as unknown };
}

// --- Instance-admin workspaces oracle helpers (NEWFRONT-444, ADM-037..045). ---

/** One workspace row as GET /api/instances/workspaces/ reports it. */
export interface ServerAdminWorkspace {
  id: string;
  name: string;
  slug: string;
  ownerEmail: string | null;
  totalProjects: number | null;
  totalMembers: number | null;
  logoUrl: string | null;
}

/** Paginated workspace list as the server reports it. */
export interface ServerAdminWorkspaceList {
  results: ServerAdminWorkspace[];
  nextPageResults: boolean;
  nextCursor: string | null;
  totalCount: number | null;
}

function toServerAdminWorkspace(row: Record<string, unknown>): ServerAdminWorkspace {
  const owner = row["owner"] as { email?: unknown } | null | undefined;
  return {
    id: typeof row["id"] === "string" ? row["id"] : "",
    name: typeof row["name"] === "string" ? row["name"] : "",
    slug: typeof row["slug"] === "string" ? row["slug"] : "",
    ownerEmail: typeof owner?.email === "string" ? owner.email : null,
    totalProjects: typeof row["total_projects"] === "number" ? row["total_projects"] : null,
    totalMembers: typeof row["total_members"] === "number" ? row["total_members"] : null,
    logoUrl: typeof row["logo_url"] === "string" && row["logo_url"] !== "" ? (row["logo_url"] as string) : null,
  };
}

/**
 * List instance workspaces as GET /api/instances/workspaces/ reports them
 * (cursor-paginated, ten per page). Throws unless the read lands.
 */
export async function serverAdminListWorkspaces(
  sessionCookie: string,
  cursor?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminWorkspaceList> {
  const url =
    cursor === undefined || cursor === ""
      ? `${apiBase}/api/instances/workspaces/`
      : `${apiBase}/api/instances/workspaces/?cursor=${encodeURIComponent(cursor)}`;
  const res = await fetch(url, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] instance workspaces read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as {
    results?: unknown[];
    next_page_results?: unknown;
    next_cursor?: unknown;
    total_count?: unknown;
  };
  const rows = Array.isArray(body.results) ? body.results : [];
  return {
    results: rows.map((row) => toServerAdminWorkspace((row ?? {}) as Record<string, unknown>)),
    nextPageResults: body.next_page_results === true,
    nextCursor: typeof body.next_cursor === "string" ? body.next_cursor : null,
    totalCount: typeof body.total_count === "number" ? body.total_count : null,
  };
}

/** Payload for creating an instance workspace (owner is server-assigned). */
export interface ServerAdminCreateWorkspaceInput {
  name: string;
  slug: string;
  organizationSize: string;
}

/**
 * Create an instance workspace via POST /api/instances/workspaces/.
 * Throws unless the server reports the created row.
 */
export async function serverAdminCreateWorkspace(
  sessionCookie: string,
  input: ServerAdminCreateWorkspaceInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminWorkspace> {
  const res = await fetch(`${apiBase}/api/instances/workspaces/`, {
    method: "POST",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify({
      name: input.name,
      slug: input.slug,
      organization_size: input.organizationSize,
    }),
  });
  if (res.status !== 201) throw new Error(`[parity] instance workspace create failed with HTTP ${res.status}.`);
  return toServerAdminWorkspace((await res.json()) as Record<string, unknown>);
}

/**
 * Check slug availability via GET /api/instances/workspace-slug-check/.
 * Resolves true when the slug is free (and not restricted). Throws unless
 * the read lands.
 */
export async function serverAdminSlugCheck(
  sessionCookie: string,
  slug: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetch(`${apiBase}/api/instances/workspace-slug-check/?slug=${encodeURIComponent(slug)}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] workspace slug check failed with HTTP ${res.status}.`);
  const body = (await res.json()) as { status?: unknown };
  return body.status === true;
}

/**
 * Delete every workspace whose slug starts with the given prefix (test
 * cleanup; the console surfaces no delete). Resolves with the deleted
 * count. Refuses an empty prefix so the seed workspace is never at risk.
 */
export async function serverAdminDeleteWorkspacesBySlugPrefix(prefix: string): Promise<number> {
  if (prefix === "") throw new Error("[parity] workspace cleanup prefix must not be empty.");
  const out = await serverAdminShell(
    `from pi_dash.db.models import Workspace\n` +
      `rows = Workspace.objects.filter(slug__startswith=${JSON.stringify(prefix)})\n` +
      `count = rows.count()\n` +
      `rows.delete()\n` +
      `print("PARITY_DELETED:" + str(count))\n` +
      `print("PARITY_DELETE_OK")\n`
  );
  if (!out.includes("PARITY_DELETE_OK")) throw new Error("[parity] workspace cleanup produced no receipt.");
  const match = /PARITY_DELETED:(\d+)/.exec(out);
  return match === null ? 0 : Number.parseInt(match[1] ?? "0", 10);
}

// --- Instance-admin loop jobs + runners server helpers (NEWFRONT-445, ADM-051..061 + ADM-080). ---

/** One loop job as the instance-admin API reports it. */
export interface ServerAdminLoopJob {
  id: string;
  slug: string;
  name: string;
  publicName: string;
  publicDescription: string;
  prompt: string;
  minRole: number;
  enabled: boolean;
  isBuiltin: boolean;
  dtstart: string | null;
  rrule: string;
  tzid: string;
  createdAt: string | null;
  updatedAt: string | null;
  /** 24h rollup; present on detail reads, absent on list reads. */
  stats: { targetCount: number; completed: number; failed: number; skipped: number } | null;
}

function toServerAdminLoopJob(row: Record<string, unknown>): ServerAdminLoopJob {
  const stats = row["stats"] as Record<string, unknown> | null | undefined;
  return {
    id: typeof row["id"] === "string" ? row["id"] : "",
    slug: typeof row["slug"] === "string" ? row["slug"] : "",
    name: typeof row["name"] === "string" ? row["name"] : "",
    publicName: typeof row["public_name"] === "string" ? row["public_name"] : "",
    publicDescription: typeof row["public_description"] === "string" ? row["public_description"] : "",
    prompt: typeof row["prompt"] === "string" ? row["prompt"] : "",
    minRole: typeof row["min_role"] === "number" ? row["min_role"] : 0,
    enabled: row["enabled"] === true,
    isBuiltin: row["is_builtin"] === true,
    dtstart: typeof row["dtstart"] === "string" ? row["dtstart"] : null,
    rrule: typeof row["rrule"] === "string" ? row["rrule"] : "",
    tzid: typeof row["tzid"] === "string" ? row["tzid"] : "",
    createdAt: typeof row["created_at"] === "string" ? row["created_at"] : null,
    updatedAt: typeof row["updated_at"] === "string" ? row["updated_at"] : null,
    stats:
      stats !== null && stats !== undefined && typeof stats["target_count"] === "number"
        ? {
            targetCount: stats["target_count"],
            completed: typeof stats["completed"] === "number" ? stats["completed"] : 0,
            failed: typeof stats["failed"] === "number" ? stats["failed"] : 0,
            skipped: typeof stats["skipped"] === "number" ? stats["skipped"] : 0,
          }
        : null,
  };
}

/** One loop target row as the instance-admin API reports it. */
export interface ServerAdminLoopTarget {
  id: string;
  workspaceSlug: string | null;
  userEmail: string | null;
  nextRunAt: string | null;
  lastSkippedAt: string | null;
  lastSkipReason: string;
  lastRun: {
    status: string;
    errorCode: string;
    modelUsed: string;
    totalTokens: number | null;
    completedAt: string | null;
  } | null;
}

function toServerAdminLoopTarget(row: Record<string, unknown>): ServerAdminLoopTarget {
  const run = row["last_run"] as Record<string, unknown> | null | undefined;
  return {
    id: typeof row["id"] === "string" ? row["id"] : "",
    workspaceSlug: typeof row["workspace_slug"] === "string" ? row["workspace_slug"] : null,
    userEmail: typeof row["user_email"] === "string" ? row["user_email"] : null,
    nextRunAt: typeof row["next_run_at"] === "string" ? row["next_run_at"] : null,
    lastSkippedAt: typeof row["last_skipped_at"] === "string" ? row["last_skipped_at"] : null,
    lastSkipReason: typeof row["last_skip_reason"] === "string" ? row["last_skip_reason"] : "",
    lastRun:
      run === null || run === undefined
        ? null
        : {
            status: typeof run["status"] === "string" ? run["status"] : "",
            errorCode: typeof run["error_code"] === "string" ? run["error_code"] : "",
            modelUsed: typeof run["model_used"] === "string" ? run["model_used"] : "",
            totalTokens: typeof run["total_tokens"] === "number" ? run["total_tokens"] : null,
            completedAt: typeof run["completed_at"] === "string" ? run["completed_at"] : null,
          },
  };
}

/**
 * List loop jobs as GET /api/instances/loop/jobs/ reports them (a bare
 * array, newest first, without stats). Throws unless the read lands.
 */
export async function serverAdminListLoopJobs(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminLoopJob[]> {
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] loop jobs read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as unknown;
  if (!Array.isArray(body)) throw new Error("[parity] loop jobs read carried no array.");
  return body.map((row) => toServerAdminLoopJob((row ?? {}) as Record<string, unknown>));
}

/**
 * Read one loop job as GET /api/instances/loop/jobs/<id>/ reports it
 * (with the 24h stats rollup). Resolves null on 404; throws otherwise
 * unless the read lands.
 */
export async function serverAdminGetLoopJob(
  sessionCookie: string,
  jobId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminLoopJob | null> {
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/${encodeURIComponent(jobId)}/`, {
    headers: { cookie: sessionCookie },
  });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`[parity] loop job read failed with HTTP ${res.status}.`);
  return toServerAdminLoopJob((await res.json()) as Record<string, unknown>);
}

/** Writable loop-job fields for create/update probes. */
export interface ServerAdminLoopJobInput {
  slug?: string;
  name?: string;
  publicName?: string;
  publicDescription?: string;
  prompt?: string;
  minRole?: number;
  enabled?: boolean;
  rrule?: string;
  tzid?: string;
}

function loopJobInputBody(input: ServerAdminLoopJobInput): Record<string, unknown> {
  const body: Record<string, unknown> = {};
  if (input.slug !== undefined) body["slug"] = input.slug;
  if (input.name !== undefined) body["name"] = input.name;
  if (input.publicName !== undefined) body["public_name"] = input.publicName;
  if (input.publicDescription !== undefined) body["public_description"] = input.publicDescription;
  if (input.prompt !== undefined) body["prompt"] = input.prompt;
  if (input.minRole !== undefined) body["min_role"] = input.minRole;
  if (input.enabled !== undefined) body["enabled"] = input.enabled;
  if (input.rrule !== undefined) body["rrule"] = input.rrule;
  if (input.tzid !== undefined) body["tzid"] = input.tzid;
  return body;
}

/**
 * Create a loop job via POST /api/instances/loop/jobs/. Throws unless the
 * server reports the created row.
 */
export async function serverAdminCreateLoopJob(
  sessionCookie: string,
  input: ServerAdminLoopJobInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminLoopJob> {
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/`, {
    method: "POST",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify(loopJobInputBody(input)),
  });
  if (res.status !== 201) throw new Error(`[parity] loop job create failed with HTTP ${res.status}.`);
  return toServerAdminLoopJob((await res.json()) as Record<string, unknown>);
}

/**
 * Update a loop job via PATCH /api/instances/loop/jobs/<id>/. Throws unless
 * the server reports the updated row.
 */
export async function serverAdminUpdateLoopJob(
  sessionCookie: string,
  jobId: string,
  input: ServerAdminLoopJobInput,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminLoopJob> {
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/${encodeURIComponent(jobId)}/`, {
    method: "PATCH",
    headers: { "content-type": "application/json", ...serverAdminCsrfHeaders(sessionCookie) },
    body: JSON.stringify(loopJobInputBody(input)),
  });
  if (!res.ok) throw new Error(`[parity] loop job update failed with HTTP ${res.status}.`);
  return toServerAdminLoopJob((await res.json()) as Record<string, unknown>);
}

/**
 * Delete a loop job via DELETE /api/instances/loop/jobs/<id>/. Throws
 * unless the server confirms.
 */
export async function serverAdminDeleteLoopJob(
  sessionCookie: string,
  jobId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/${encodeURIComponent(jobId)}/`, {
    method: "DELETE",
    headers: { ...serverAdminCsrfHeaders(sessionCookie) },
  });
  if (res.status !== 204 && res.status !== 200) {
    throw new Error(`[parity] loop job delete failed with HTTP ${res.status}.`);
  }
}

/** One page of loop targets as the instance-admin API reports it. */
export interface ServerAdminLoopTargetsPage {
  page: number;
  results: ServerAdminLoopTarget[];
}

/**
 * List a job's targets as GET /api/instances/loop/jobs/<id>/targets/
 * reports them. Throws unless the read lands.
 */
export async function serverAdminListLoopTargets(
  sessionCookie: string,
  jobId: string,
  params: { page?: number; skipReason?: string } = {},
  apiBase: string = apiBaseFromEnv()
): Promise<ServerAdminLoopTargetsPage> {
  const query = new URLSearchParams();
  if (params.page !== undefined) query.set("page", String(params.page));
  if (params.skipReason !== undefined && params.skipReason !== "") query.set("skip_reason", params.skipReason);
  const suffix = query.toString() === "" ? "" : `?${query.toString()}`;
  const res = await fetch(`${apiBase}/api/instances/loop/jobs/${encodeURIComponent(jobId)}/targets/${suffix}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] loop targets read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as { page?: unknown; results?: unknown[] };
  const rows = Array.isArray(body.results) ? body.results : [];
  return {
    page: typeof body.page === "number" ? body.page : 1,
    results: rows.map((row) => toServerAdminLoopTarget((row ?? {}) as Record<string, unknown>)),
  };
}

/**
 * Delete every loop job whose slug starts with the given prefix (test
 * cleanup). Resolves with the deleted count. Refuses an empty prefix so
 * the seeded builtin job is never at risk.
 */
export async function serverAdminDeleteLoopJobsBySlugPrefix(prefix: string): Promise<number> {
  if (prefix === "") throw new Error("[parity] loop job cleanup prefix must not be empty.");
  const out = await serverAdminShell(
    `from pi_dash.db.models import LoopJob\n` +
      `rows = LoopJob.objects.filter(slug__startswith=${JSON.stringify(prefix)})\n` +
      `count = rows.count()\n` +
      `rows.delete()\n` +
      `print("PARITY_DELETED:" + str(count))\n` +
      `print("PARITY_DELETE_OK")\n`
  );
  if (!out.includes("PARITY_DELETE_OK")) throw new Error("[parity] loop job cleanup produced no receipt.");
  const match = /PARITY_DELETED:(\d+)/.exec(out);
  return match === null ? 0 : Number.parseInt(match[1] ?? "0", 10);
}

/** Input for creating one loop target edge (test fixture). */
export interface ServerAdminLoopTargetInput {
  jobSlug: string;
  workspaceSlug: string;
  userEmail: string;
  /** Hours from now for next_run_at; null leaves it unset. Defaults to 1. */
  nextRunInHours?: number | null;
  /** Skip reason to record; "" leaves the edge unskipped. Defaults to "". */
  skipReason?: string;
  /** Stamp last_skipped_at now when a reason is given. Defaults to true. */
  skippedNow?: boolean;
}

/**
 * Create one loop target edge (job x workspace x user) via the Django
 * shell — targets have no write API; reconcile owns them in production.
 * Replaces any live edge for the same triple so retries converge.
 * Resolves with the target id. Throws unless the create lands.
 */
export async function serverAdminCreateLoopTarget(input: ServerAdminLoopTargetInput): Promise<string> {
  const nextRun = input.nextRunInHours === undefined ? 1 : input.nextRunInHours;
  const reason = input.skipReason ?? "";
  const skippedNow = input.skippedNow ?? true;
  const out = await serverAdminShell(
    `from datetime import timedelta\n` +
      `from django.utils import timezone\n` +
      `from pi_dash.db.models import LoopJob, LoopTarget, User, Workspace\n` +
      `job = LoopJob.objects.get(slug=${JSON.stringify(input.jobSlug)}, deleted_at__isnull=True)\n` +
      `ws = Workspace.objects.get(slug=${JSON.stringify(input.workspaceSlug)})\n` +
      `user = User.objects.get(email=${JSON.stringify(input.userEmail)})\n` +
      `LoopTarget.objects.filter(job=job, workspace=ws, user=user, deleted_at__isnull=True).delete()\n` +
      `at = (timezone.now() + timedelta(hours=${JSON.stringify(nextRun ?? 0)})) if ${nextRun === null ? "False" : "True"} else None\n` +
      `skipped = timezone.now() if (${JSON.stringify(reason)} != "" and ${skippedNow ? "True" : "False"}) else None\n` +
      `t = LoopTarget.objects.create(job=job, workspace=ws, user=user, next_run_at=at, last_skip_reason=${JSON.stringify(reason)}, last_skipped_at=skipped)\n` +
      `print("PARITY_TARGET:" + str(t.id))\n` +
      `print("PARITY_TARGET_OK")\n`
  );
  if (!out.includes("PARITY_TARGET_OK")) throw new Error("[parity] loop target create produced no receipt.");
  const match = /PARITY_TARGET:([0-9a-f-]+)/.exec(out);
  if (match === null) throw new Error("[parity] loop target create carried no id.");
  return match[1] ?? "";
}

/**
 * Delete every loop target whose job slug starts with the given prefix
 * (test cleanup). Resolves with the deleted count. Refuses an empty
 * prefix so seeded edges are never at risk.
 */
export async function serverAdminDeleteLoopTargetsByJobSlugPrefix(prefix: string): Promise<number> {
  if (prefix === "") throw new Error("[parity] loop target cleanup prefix must not be empty.");
  const out = await serverAdminShell(
    `from pi_dash.db.models import LoopTarget\n` +
      `rows = LoopTarget.objects.filter(job__slug__startswith=${JSON.stringify(prefix)})\n` +
      `count = rows.count()\n` +
      `rows.delete()\n` +
      `print("PARITY_DELETED:" + str(count))\n` +
      `print("PARITY_DELETE_OK")\n`
  );
  if (!out.includes("PARITY_DELETE_OK")) throw new Error("[parity] loop target cleanup produced no receipt.");
  const match = /PARITY_DELETED:(\d+)/.exec(out);
  return match === null ? 0 : Number.parseInt(match[1] ?? "0", 10);
}

/**
 * Probe the instance runners endpoint (GET /api/instances/runners/).
 * Resolves with the raw HTTP status so absence pins can assert it
 * without throwing. Throws only when the request itself fails.
 */
export async function serverAdminRunnersEndpointStatus(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<number> {
  const res = await fetch(`${apiBase}/api/instances/runners/`, { headers: { cookie: sessionCookie } });
  return res.status;
}

// --- Instance-admin shell + cloud entry oracle (NEWFRONT-446, ADM-062..079).
// --- Appended; existing helpers above are untouched per the shared harness contract.

/** Running instance version as GET /api/instances/ reports it (help footer, ADM-067). */
export async function serverAdminInstanceVersion(apiBase: string = apiBaseFromEnv()): Promise<string> {
  const res = await fetchTolerant(`${apiBase}/api/instances/`);
  if (!res.ok) throw new Error(`[parity] instance info read failed with HTTP ${res.status}.`);
  const body = (await res.json()) as { instance?: Record<string, unknown> };
  const version = body.instance?.["current_version"];
  if (typeof version !== "string" || version === "") {
    throw new Error("[parity] instance info carried no current_version.");
  }
  return version;
}

/**
 * Grant instance-admin to a cloud user (ADM-075 fixture): the same
 * get_or_create the cloud_grant_admin command runs, through the cloud
 * api container. Fresh dev-login emails keep fixtures per-scenario, so
 * no restore is needed. Resolves with whether the row was created.
 */
export async function serverAdminCloudGrantAdmin(email: string): Promise<boolean> {
  const out = await cloudShell(
    `from pi_dash.db.models import User\n` +
      `from pi_dash.license.models import Instance, InstanceAdmin\n` +
      `user = User.objects.get(email=${JSON.stringify(email)})\n` +
      `instance = Instance.objects.first()\n` +
      `_, created = InstanceAdmin.objects.get_or_create(user=user, instance=instance, defaults={"role": 20, "is_verified": True})\n` +
      `print("PARITY_GRANTED:" + ("1" if created else "0"))\n` +
      `print("PARITY_GRANT_OK")\n`
  );
  if (!out.includes("PARITY_GRANT_OK")) throw new Error(`[parity] cloud admin grant produced no receipt for ${email}.`);
  return /^PARITY_GRANTED:1$/m.test(out);
}

/** Cloud auth-probe check: status plus user id when the cookie carries a session. */
export async function serverCloudAuthMe(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; userId: string | null }> {
  const res = await fetch(`${apiBase}/api/auth/me/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) return { status: res.status, userId: null };
  const row = (await res.json()) as { id?: unknown; user?: { id?: unknown } };
  const raw = typeof row.id === "string" ? row.id : typeof row.user?.id === "string" ? row.user.id : null;
  return { status: res.status, userId: raw };
}

/**
 * Cloud password-auth shadow check (ADM-074 server half): the OSS
 * email/password admin endpoints answer 403 in the cloud edition (OIDC
 * is the only way in). Resolves with both statuses.
 */
export async function serverCloudAdminPasswordShadowed(
  apiBase: string = apiBaseFromEnv()
): Promise<{ signIn: number; signUp: number }> {
  // A valid CSRF token carries the posts past the middleware so they
  // reach the shadow view (without one, the CSRF failure page answers
  // before any view runs).
  const tokenRes = await fetchTolerant(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  const cookies = cookieHeader(setCookieHeaders(tokenRes));
  const post = (path: string): Promise<Response> =>
    fetch(`${apiBase}${path}`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", cookie: cookies },
      body: new URLSearchParams({
        email: "nobody@example.com",
        password: "Parity-Nope-1!",
        csrfmiddlewaretoken: token,
      }),
      redirect: "manual",
    });
  const signIn = await post("/api/instances/admins/sign-in/");
  const signUp = await post("/api/instances/admins/sign-up/");
  return { signIn: signIn.status, signUp: signUp.status };
}

// --- Personal access tokens list + full-field create (NEWFRONT-413,
// --- WSET-025/026/027/028). Appended; existing helpers above are untouched
// --- per the shared harness contract. serverMintApiToken and
// --- serverDeleteApiToken stay as-is; the list read and the expiring
// --- create are new. Tokens are per-user, so every helper takes the token
// --- owner's session.

/** One personal-access-token row as the user token endpoints serialize it. */
export interface ParityUserApiToken {
  id: string;
  label: string;
  description: string;
  expiredAt: string | null;
  active: boolean;
  /** Whether the payload carried the raw secret (only create responses do). */
  hasToken: boolean;
}

function userApiTokenOf(row: Record<string, unknown>): ParityUserApiToken {
  if (typeof row["id"] !== "string" || typeof row["label"] !== "string") {
    throw new Error("[parity] api-token row carried no string id/label.");
  }
  const expired = row["expired_at"];
  return {
    id: row["id"],
    label: row["label"],
    description: typeof row["description"] === "string" ? row["description"] : "",
    expiredAt: typeof expired === "string" ? expired : null,
    active: row["is_active"] === true,
    hasToken: typeof row["token"] === "string",
  };
}

/** Tokens the session owner owns, in server order. */
export async function serverApiTokens(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityUserApiToken[]> {
  const res = await authedApi(`/users/api-tokens/`, sessionCookie, undefined, apiBase);
  requireOk(res, "api-token list");
  const rows = (await res.json()) as Array<Record<string, unknown>>;
  return rows.map(userApiTokenOf);
}

/** Create with full fields; resolves with the id plus the raw one-time secret. */
export async function serverCreateApiToken(
  sessionCookie: string,
  input: { label: string; description?: string; expiredAt?: string | null },
  apiBase: string = apiBaseFromEnv()
): Promise<{ id: string; token: string }> {
  const body: Record<string, unknown> = { label: input.label };
  if (input.description !== undefined) body["description"] = input.description;
  if (input.expiredAt !== undefined) body["expired_at"] = input.expiredAt;
  const res = await authedApi(
    `/users/api-tokens/`,
    sessionCookie,
    { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
    apiBase
  );
  requireOk(res, "api-token create");
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["id"] !== "string" || typeof rec["token"] !== "string") {
    throw new Error("[parity] api-token create response carried no id/token.");
  }
  return { id: rec["id"], token: rec["token"] };
}

// --- Project settings General tab (NEWFRONT-389, PSET-005–012). Appended;
// --- existing helpers above are untouched per the shared harness contract.

/** Raw project detail record; specs pick the fields their row needs. */
export async function serverProjectRecord(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

/** Project detail fetch without throwing: HTTP status plus the record when present. */
export async function serverProjectFetchStatus(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; record: Record<string, unknown> | null }> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) return { status: res.status, record: null };
  return { status: res.status, record: (await res.json()) as Record<string, unknown> };
}

/** Raw project DELETE: resolves with the HTTP status (refusals carry the body). */
export async function serverDeleteProjectRaw(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: string }> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    method: "DELETE",
    headers: { cookie: sessionCookie },
  });
  return { status: res.status, body: await res.text() };
}

/** Whether `identifier` is taken in the workspace (the live precheck behind the General tab). */
export async function serverIdentifierTaken(
  workspaceSlug: string,
  identifier: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const params = new URLSearchParams({ name: identifier });
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/project-identifiers?${params.toString()}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] identifier lookup failed with HTTP ${res.status}.`);
  // The endpoint answers a reservation COUNT, not a boolean.
  const payload = (await res.json()) as { exists?: unknown };
  return Number(payload.exists ?? 0) > 0;
}

/** Raw git-repository binding status of a project. */
export async function serverRepositoryStatus(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] repository read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
}

/** Bind a repository URL; resolves with the raw status and body text (refusal pins). */
export async function serverBindRepositoryRaw(
  workspaceSlug: string,
  projectId: string,
  repoUrl: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number; body: string }> {
  const res = await fetchTolerant(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/bind/`, {
    method: "POST",
    headers: { cookie: sessionCookie, "content-type": "application/json" },
    body: JSON.stringify({ repo_url: repoUrl }),
  });
  return { status: res.status, body: await res.text() };
}

/** Issue names the project search endpoint returns for `query`. */
export async function serverSearchIssueNames(
  workspaceSlug: string,
  projectId: string,
  query: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const params = new URLSearchParams({ search: query, workspace_search: "false" });
  const res = await fetchTolerant(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/search-issues/?${params.toString()}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] issue search failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : [];
  return rows
    .map((row) => (row as Record<string, unknown>)["name"])
    .filter((name): name is string => typeof name === "string");
}

// ---------------------------------------------------------------------------
// Profile preferences + own activity reads (NEWFRONT-427, PROF-016..019).
// Theme, locale and own-activity state as the server knows them, backing the
// preferences and own-activity oracle scenarios. Appended; existing helpers
// above are untouched per the shared contract.
// ---------------------------------------------------------------------------

/** Theme object the profile endpoint stores (NEWFRONT-427, PROF-016). */
export interface ServerProfileTheme {
  theme?: string;
  primary?: string;
  background?: string;
  darkPalette?: boolean;
}

/** Signed-in user's theme object, as the server knows it. */
export async function serverProfileTheme(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerProfileTheme> {
  const res = await fetch(`${apiBase}/api/users/me/profile/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as { theme?: unknown };
  const theme = (row.theme ?? {}) as Record<string, unknown>;
  const out: ServerProfileTheme = {};
  if (typeof theme["theme"] === "string") out.theme = theme["theme"] as string;
  if (typeof theme["primary"] === "string") out.primary = theme["primary"] as string;
  if (typeof theme["background"] === "string") out.background = theme["background"] as string;
  if (typeof theme["darkPalette"] === "boolean") out.darkPalette = theme["darkPalette"] as boolean;
  return out;
}

/** Signed-in user's locale choices, as the server knows them (NEWFRONT-427, PROF-017). */
export interface ServerProfileLocale {
  language?: string;
  start_of_the_week?: number;
  user_timezone?: string;
}

/** Language plus week start from the profile, timezone from the user record. */
export async function serverProfileLocale(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerProfileLocale> {
  const [profileRes, userRes] = await Promise.all([
    fetch(`${apiBase}/api/users/me/profile/`, { headers: { cookie: sessionCookie } }),
    fetch(`${apiBase}/api/users/me/`, { headers: { cookie: sessionCookie } }),
  ]);
  if (!profileRes.ok) throw new Error(`[parity] profile read failed with HTTP ${profileRes.status}.`);
  if (!userRes.ok) throw new Error(`[parity] users/me read failed with HTTP ${userRes.status}.`);
  const profile = (await profileRes.json()) as Record<string, unknown>;
  const user = (await userRes.json()) as Record<string, unknown>;
  const out: ServerProfileLocale = {};
  if (typeof profile["language"] === "string") out.language = profile["language"];
  if (typeof profile["start_of_the_week"] === "number") out.start_of_the_week = profile["start_of_the_week"];
  if (typeof user["user_timezone"] === "string") out.user_timezone = user["user_timezone"];
  return out;
}

/** One page of the own-activity feed (NEWFRONT-427, PROF-019). */
export interface ServerOwnActivityPage {
  results: unknown[];
  total_pages: number;
  total_results: number;
}

/** Own cross-workspace activity through the endpoint the settings tab reads. */
export async function serverOwnActivities(
  sessionCookie: string,
  cursor?: string,
  perPage = 100,
  apiBase: string = apiBaseFromEnv()
): Promise<ServerOwnActivityPage> {
  const params = new URLSearchParams({ per_page: String(perPage) });
  if (cursor !== undefined) params.set("cursor", cursor);
  const res = await fetch(`${apiBase}/api/users/me/activities/?${params.toString()}`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] own-activity read failed with HTTP ${res.status}.`);
  const row = (await res.json()) as Record<string, unknown>;
  return {
    results: Array.isArray(row["results"]) ? row["results"] : [],
    total_pages: typeof row["total_pages"] === "number" ? row["total_pages"] : 0,
    total_results: typeof row["total_results"] === "number" ? row["total_results"] : 0,
  };
}

// --- Intake list shell (NEWFRONT-255, INT-001-005/008/012). Appended;
// --- existing helpers above are untouched per the shared parity contract.
// --- The intake read endpoints take the nested issue id; the feature flag
// --- reads and writes as `inbox_view` on the project (an alias over the
// --- intake-view column).

/** Whether the project's intake view is switched on. */
export async function serverIntakeShellInboxView(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<boolean> {
  const res = await fetchShared(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`, {
    headers: { cookie: sessionCookie },
  });
  if (!res.ok) throw new Error(`[parity] project read failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  return rec["inbox_view"] === true;
}

/** Switch the project's intake view on or off (the view maps the alias onto its column). */
export async function serverIntakeShellSetInboxView(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  on: boolean,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/`,
    sessionCookie,
    { inbox_view: on }
  );
  if (!res.ok) throw new Error(`[parity] project intake-view patch failed with HTTP ${res.status}.`);
}

/** Intake fields a scenario may set on an inbox row (status moves, snooze dates). */
export interface ServerIntakeShellInboxPatch {
  status?: number;
  snoozed_till?: string | null;
  duplicate_to?: string;
}

/** Patch an inbox row by its nested issue id; resolves with the stored intake status. */
export async function serverIntakeShellPatchInboxIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  patch: ServerIntakeShellInboxPatch,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ status: number }> {
  const res = await mutateJSON(
    "PATCH",
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${issueId}/`,
    sessionCookie,
    patch
  );
  if (!res.ok) throw new Error(`[parity] inbox-issue patch failed with HTTP ${res.status}.`);
  const rec = (await res.json()) as Record<string, unknown>;
  if (typeof rec["status"] !== "number") throw new Error("[parity] patched inbox issue carried no numeric status.");
  return { status: rec["status"] as number };
}

/** One inbox row with its intake status, as the list endpoint reports it. */
export interface ServerIntakeShellInboxRow {
  issueId: string;
  name: string;
  status: number;
}

/**
 * Inbox rows on a project, optionally narrowed to a status query (the
 * comma-joined form the UI sends, e.g. "-2" or "1,-1,2"; omitted to let
 * the server apply its own default). Resolves with the rows plus the
 * collection total the pending badge mirrors.
 */
export async function serverIntakeShellInboxIssues(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  statusQuery?: string,
  apiBase: string = apiBaseFromEnv()
): Promise<{ rows: ServerIntakeShellInboxRow[]; total: number }> {
  const query = statusQuery === undefined ? "" : `?status=${encodeURIComponent(statusQuery)}`;
  const res = await fetchShared(
    `${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/inbox-issues/${query}`,
    { headers: { cookie: sessionCookie } }
  );
  if (!res.ok) throw new Error(`[parity] inbox-issues read failed with HTTP ${res.status}.`);
  const payload: unknown = await res.json();
  const rows: unknown[] = Array.isArray(payload) ? payload : ((payload as { results?: unknown[] }).results ?? []);
  const total =
    !Array.isArray(payload) && typeof (payload as { total_results?: unknown }).total_results === "number"
      ? (payload as { total_results: number }).total_results
      : rows.length;
  return {
    rows: rows.map((row) => {
      const rec = row as Record<string, unknown>;
      const issue = rec["issue"] as Record<string, unknown> | undefined;
      return {
        issueId: typeof issue?.["id"] === "string" ? (issue["id"] as string) : "",
        name: typeof issue?.["name"] === "string" ? (issue["name"] as string) : "",
        status: typeof rec["status"] === "number" ? (rec["status"] as number) : 0,
      };
    }),
    total,
  };
}

// --- NEWFRONT-525: one folder's nested favorites, as the folder's own
// --- grouped read serves them. Appended; existing helpers above are
// --- untouched per the shared parity contract.

/** Children nested under one favorites folder. */
export async function listFavoriteGroup(
  workspaceSlug: string,
  sessionCookie: string,
  folderId: string,
  apiBase: string = apiBaseFromEnv()
): Promise<ParityFavorite[]> {
  const res = await apiJson(
    "GET",
    `/api/workspaces/${workspaceSlug}/user-favorites/${folderId}/group/`,
    sessionCookie,
    undefined,
    apiBase
  );
  if (res.status !== 200) throw new Error(`[parity] favorite group read failed with HTTP ${res.status}.`);
  const rows: unknown[] = Array.isArray(res.payload)
    ? res.payload
    : ((res.payload as { results?: unknown[] }).results ?? []);
  return rows as ParityFavorite[];
}

// --- Nested pages (NEWFRONT-850, PAGE-084-090). Appended; existing
// --- helpers above are untouched per the shared parity contract. The
// --- nested surface lands with PDASHOSS01-270; these write the same
// --- `parent` field the spec sketch defines and the UI sends.

/** Reparent one page to `parentId`, or to the top level with null. */
export async function serverPatchPageParent(
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  parentId: string | null,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<void> {
  const res = await apiJson(
    "PATCH",
    `/api/workspaces/${workspaceSlug}/projects/${projectId}/pages/${pageId}/`,
    sessionCookie,
    { parent: parentId },
    apiBase
  );
  if (res.status !== 200) {
    throw new Error(
      `[parity] page parent patch failed with HTTP ${res.status}: ${JSON.stringify(res.payload).slice(0, 300)}.`
    );
  }
}
