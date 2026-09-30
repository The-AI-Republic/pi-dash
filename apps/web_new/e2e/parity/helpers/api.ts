// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Server-state assertions for parity scenarios (NEWFRONT-19). The driver
// proves what the user sees; this helper proves what the server stored, so
// a redesigned screen cannot pass while saving the wrong thing. It signs
// in through the same native credential endpoint the old sign-in card
// posts to, then reads back through the public REST API with that session.
import { readFileSync } from "node:fs";
import type { ParitySeedFacts } from "../drivers/parity-driver";

function apiBaseFromEnv(): string {
  const raw = (process.env["PARITY_API_URL"] ?? "http://localhost:18019").trim().replace(/\/+$/, "");
  return raw;
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

/** Sign in with email plus password; resolves with a session cookie header. */
export async function signInSession(
  email: string,
  password: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string> {
  // The credential endpoint is a native form POST guarded by CSRF, so fetch
  // a token first exactly like the sign-in card does, then submit the form.
  const tokenRes = await fetch(`${apiBase}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] CSRF token fetch failed with HTTP ${tokenRes.status}.`);
  const tokenPayload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof tokenPayload.csrf_token === "string" ? tokenPayload.csrf_token : "";
  if (token === "") throw new Error("[parity] CSRF token response carried no token.");
  const preCookies = cookieHeader(setCookieHeaders(tokenRes));
  const body = new URLSearchParams({ email, password, csrfmiddlewaretoken: token });
  const res = await fetch(`${apiBase}/auth/sign-in/`, {
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
}

/** Names of the project's issues as the server reports them, in API order. */
export async function serverIssueNames(
  workspaceSlug: string,
  projectId: string,
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<string[]> {
  const res = await fetch(`${apiBase}/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/`, {
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
export interface ProjectFacts {
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
  return fetch(`${session.apiBase}${path}`, {
    method,
    headers,
    body: jsonBody === undefined ? undefined : JSON.stringify(jsonBody),
  });
}

/** The project collection the list screen reads (`projects/details/`). */
export async function projectsDetails(session: AuthedSession, slug: string): Promise<ProjectFacts[]> {
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
): Promise<ProjectFacts | undefined> {
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
