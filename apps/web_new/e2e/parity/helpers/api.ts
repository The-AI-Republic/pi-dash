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

/** The project's short-code identifier (e.g. "PARI"), read from the server. */
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
  if (typeof payload.identifier !== "string" || payload.identifier.length === 0) {
    throw new Error("[parity] project payload carried no identifier.");
  }
  return payload.identifier;
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

/** The signed-in user's profile (theme, language, start_of_the_week, …). */
export async function serverUserProfile(
  sessionCookie: string,
  apiBase: string = apiBaseFromEnv()
): Promise<Record<string, unknown>> {
  const res = await fetch(`${apiBase}/api/users/me/profile/`, { headers: { cookie: sessionCookie } });
  if (!res.ok) throw new Error(`[parity] profile read failed with HTTP ${res.status}.`);
  return (await res.json()) as Record<string, unknown>;
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
