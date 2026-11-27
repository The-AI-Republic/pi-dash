// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The sign-in reconciliation (desktop_unified_signin §6.2). The newest event
// wins, whichever side made it and whichever account it names.
//
// Pure, so every outcome is tested without a daemon: given the app's record,
// the daemon's `Hello` and the app's current session, this decides the one
// action the supervisor takes. The supervisor performs the action, advances
// `last_seen_seq` (no echo, rule 2), and records `own_event` on app-side
// transitions.

import type { AuthSyncEvent, AuthSyncRecord, DaemonAuthEvent, DaemonHello } from "../../../core/platform/index.js";

export interface SyncMe {
  id: string;
  email?: string | undefined;
  cloudUrl: string;
}

/** The one action a check takes. The supervisor performs it. */
export type SyncDecision =
  | { kind: "none" }
  | { kind: "handoff"; userId: string }
  | { kind: "sign_in_cli"; force: boolean; switch: boolean }
  | { kind: "sign_out_app" }
  | { kind: "sign_out_cli" }
  | { kind: "re_sign_in_cli" };

export interface SyncSnapshot {
  record: AuthSyncRecord | null;
  daemon: DaemonHello | null;
  me: SyncMe | null;
  /** The cloud the app is built for (its API origin). */
  appCloudUrl: string;
}

/** Compare `at` stamps (both this computer's clock); unknown or tied goes to the daemon. */
function daemonNewer(daemonAt: string | undefined, ownAt: string | undefined): boolean {
  const daemonMs = daemonAt === undefined ? Number.NaN : Date.parse(daemonAt);
  const ownMs = ownAt === undefined ? Number.NaN : Date.parse(ownAt);
  if (Number.isNaN(daemonMs) || Number.isNaN(ownMs)) return true;
  return !(ownMs > daemonMs);
}

/** Same person: the user id decides when both have one, e-mail (case-insensitive) otherwise. */
export function sameAccount(
  me: SyncMe | null,
  account: { user_id?: string | undefined; email?: string | undefined } | undefined
): boolean {
  if (!me || !account) return false;
  if (account.user_id && account.user_id !== "" && me.id !== "") return account.user_id === me.id;
  if (account.email && account.email !== "" && me.email) {
    return account.email.trim().toLowerCase() === String(me.email).trim().toLowerCase();
  }
  if (!account.user_id && !account.email) return true;
  return false;
}

/** Same cloud: scheme, host and port, whatever the path or a trailing slash. */
export function sameCloud(a: string, b: string): boolean {
  try {
    return new URL(a.trim()).origin === new URL(b.trim()).origin;
  } catch {
    return a.trim().replace(/\/+$/, "").toLowerCase() === b.trim().replace(/\/+$/, "").toLowerCase();
  }
}

const SIGNED_IN = "signed_in";
const SIGNED_OUT = "signed_out";

/** A sign-out the user made (rule 1): only these sign the app out. */
function isUserSignOut(reason: string | undefined): boolean {
  return reason === "logout" || reason === "last_workspace_logout" || reason === "removed";
}

/**
 * Decide the one action for this check (design §6.2). `daemon` is the raw
 * `Hello` (`null`: no answer, no daemon, or incompatible — the comparison is
 * skipped and the record kept). `me` is the live session (`null` when
 * signed out).
 */
export function reconcile(snapshot: SyncSnapshot): SyncDecision {
  const { record, daemon, me, appCloudUrl } = snapshot;
  if (!daemon) return { kind: "none" };
  const own = record?.own_event ?? null;
  const lastSeen = record?.last_seen_seq ?? null;
  const event = daemon.auth_event ?? null;

  if (event !== null) {
    return reconcileWithEvent({ daemon, own, me, event, lastSeen, appCloudUrl });
  }
  return reconcileWithoutEvent({ daemon, own, me, appCloudUrl });
}

interface WithEvent {
  daemon: DaemonHello;
  own: AuthSyncEvent | null;
  me: SyncMe | null;
  event: DaemonAuthEvent;
  lastSeen: number | null;
  appCloudUrl: string;
}

function reconcileWithEvent({ daemon, own, me, event, lastSeen, appCloudUrl }: WithEvent): SyncDecision {
  // A daemon event the app has already seen is never newer than the app's
  // own (rule: ordering). An expired or revoked daemon credential is not a
  // user's action, so it never wins either: the app signs the CLI in again
  // when it is itself signed in.
  const unseen = lastSeen === null || event.seq > lastSeen;
  if (event.kind === SIGNED_OUT && (event.reason === "expired" || event.reason === "revoked")) {
    return own?.kind === SIGNED_IN && me ? { kind: "re_sign_in_cli" } : { kind: "none" };
  }
  if (unseen && daemonNewer(event.at, own?.at)) {
    // The daemon's event is the newest: the app follows — except across
    // clouds: the app cannot follow a daemon signed in to another cloud
    // (§6.2); it stays, with its engine features off.
    if (!sameCloud(appCloudUrl, daemon.cloud_url)) return { kind: "none" };
    if (event.kind === SIGNED_IN) {
      if (me && sameAccount(me, event.account)) return { kind: "none" };
      if (!event.account?.user_id) return { kind: "none" };
      return { kind: "handoff", userId: event.account.user_id };
    }
    if (isUserSignOut(event.reason)) return me ? { kind: "sign_out_app" } : { kind: "none" };
    return { kind: "none" };
  }
  // The app's event is the newest (or the daemon's was already acted on):
  // the CLI follows the app's.
  return cliFollowsOwn(daemon, own, me);
}

/** The app's event won (or stands alone): the CLI follows it. */
function cliFollowsOwn(daemon: DaemonHello, own: AuthSyncEvent | null, me: SyncMe | null): SyncDecision {
  if (own === null) return { kind: "none" };
  const daemonAccount = daemon.account;
  if (own.kind === SIGNED_OUT) {
    // A failed CLI logout is retried here, never undone (rule 2).
    return daemonAccount ? { kind: "sign_out_cli" } : { kind: "none" };
  }
  // own signed in: the daemon follows. Another cloud means `--force`
  // (the app switches the daemon to its cloud, §6.2); another account
  // means an account switch (§6.4).
  if (!me) return { kind: "none" };
  const otherCloud = !sameCloud(me.cloudUrl, daemon.cloud_url);
  if (daemonAccount && !sameAccount(me, daemonAccount)) {
    return { kind: "sign_in_cli", force: otherCloud, switch: !otherCloud };
  }
  if (daemonAccount) return { kind: "none" };
  return { kind: "sign_in_cli", force: otherCloud, switch: false };
}

function reconcileWithoutEvent({
  daemon,
  own,
  me,
  appCloudUrl,
}: {
  daemon: DaemonHello;
  own: AuthSyncEvent | null;
  me: SyncMe | null;
  appCloudUrl: string;
}): SyncDecision {
  // No `seq` (a daemon from before `auth_event`, design §8): with no event
  // of the app's own, the daemon's present state is the newest event and it
  // wins. Once the app has an event of its own, a signed-out daemon no
  // longer beats a fresh app sign-in — otherwise the two could never
  // converge with a never-updated daemon — while a daemon signed in as
  // someone else still wins (CLI-first is the common case).
  const daemonAccount = daemon.account;
  if (own === null) {
    if (!sameCloud(appCloudUrl, daemon.cloud_url)) return { kind: "none" };
    if (daemonAccount) {
      return daemonAccount.user_id ? { kind: "handoff", userId: daemonAccount.user_id } : { kind: "none" };
    }
    return me ? { kind: "sign_out_app" } : { kind: "none" };
  }
  if (own.kind === SIGNED_IN) {
    if (!me) return { kind: "none" };
    if (!daemonAccount) {
      return cliFollowsOwn(daemon, own, me);
    }
    if (sameAccount(me, daemonAccount)) return { kind: "none" };
    if (!sameCloud(appCloudUrl, daemon.cloud_url)) return { kind: "none" };
    return daemonAccount.user_id ? { kind: "handoff", userId: daemonAccount.user_id } : { kind: "none" };
  }
  // own signed out.
  if (!sameCloud(appCloudUrl, daemon.cloud_url)) return { kind: "none" };
  if (daemonAccount) {
    return daemonAccount.user_id ? { kind: "handoff", userId: daemonAccount.user_id } : { kind: "none" };
  }
  return { kind: "none" };
}
