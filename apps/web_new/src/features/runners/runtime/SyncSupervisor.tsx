// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in reconciliation supervisor (desktop_unified_signin §6.2). Mounted
// once at the app root, outside the signed-in workspace layout, so it runs
// on the sign-in screen too — a CLI-first sign-in has no user yet, and the
// supervisor has no `userId` to gate on (unlike `RuntimeSupervisor`, which
// returns early without one).
//
// Every daemon check (launch, focus, network recovery, every minute) it
// compares two events — the app's own last sign-in or sign-out and the
// daemon's `auth_event` — and the newest one wins, whichever side made it
// and whichever account it names (U-2). A daemon the app cannot reach
// answers nothing, and the comparison is skipped (§6.2).
//
// - daemon signed in newest: the app follows through the handoff (§5),
//   signing in with no browser; when the handoff is unavailable (an old
//   daemon, the kill switch, Windows), it asks for the browser sign-in the
//   user is already looking at, once per account;
// - daemon signed-out newest (a user's sign-out): the app signs itself out
//   keeping chat history, with the signed-out notice;
// - app signed in newest, daemon not: the CLI follows at once through
//   `signInDaemonForAccount` (an account switch parks and restarts);
// - app signed out newest, daemon still signed in: `pidash auth logout`
//   again, so a logout that failed is retried, never undone.
// An account switch with runs in flight asks first (§6.4) and switches only
// on confirm; runs end as failed and pinned runs go back to their pod.
// Cancelling marks the daemon's event seen, so the check does not re-ask
// until the risk changes (new runs) — the app keeps working on its own
// until then.

import { useRouter } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type AgentRuntimeApi, type AuthSyncRecord, type DaemonHello } from "../../../core/platform/index.js";
import { resetRegisteredStores, sessionKeys, useMe, useSessionStore } from "../../../core/session/index.js";
import { notifyInfo } from "../../../shared/shell/index.js";
import { performSignOut } from "../../auth/index.js";
import { daemonSetupInProgress } from "./service.js";
import { reconcile, sameAccount, type SyncDecision, type SyncMe } from "./sync.js";
import type { SwitchWarningCopy } from "./SwitchWarningDialog.js";

/** Background re-check cadence (plus every window focus). */
export const AUTH_SYNC_INTERVAL_MS = 60_000;

/** The signed-out notice when the CLI signed the app out (§4). */
export const CLI_SIGN_OUT_TITLE = "Signed out from this computer";
export const CLI_SIGN_OUT_MESSAGE = "You were signed out because Pi Dash on this computer was signed out.";

/** How long a fresh-sign-in marker stays fresh (sign-in, then restore). */
const FRESH_SIGN_IN_WINDOW_MS = 10 * 60_000;

function nowIso(): string {
  return new Date().toISOString();
}

// The dialog chunk stays out of this module's chunk; it loads on first use.
const LazySwitchWarningDialog = React.lazy(() =>
  import("./SwitchWarningDialog.js").then((module) => ({ default: module.SwitchWarningDialog }))
);

interface PendingWarning {
  copy: SwitchWarningCopy;
  /** The daemon `seq` the choice marks seen when the user cancels. */
  daemonSeq: number | null;
  /** The deferred key for this exact check (risk changed past it asks again). */
  key: string;
  proceed: () => void;
}

/**
 * The reconciliation supervisor. Desktop only: on web and cloud the
 * platform exposes no runtime capability, so this renders nothing and runs
 * no step (and never touches the clients in that shape).
 */
export function AuthSyncSupervisor(): React.ReactElement | null {
  const runtime = platform.agentRuntime;
  if (!runtime) return null;
  return <AuthSyncSupervisorInner runtime={runtime} />;
}

function AuthSyncSupervisorInner({ runtime }: { runtime: AgentRuntimeApi }): React.ReactElement | null {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const router = useRouter();
  const { data: meData } = useMe();
  const [switchWarning, setSwitchWarning] = React.useState<PendingWarning | null>(null);
  const ticking = React.useRef(false);
  /** `daemon-seq:own-at:active` the user already cancelled: don't re-ask. */
  const deferred = React.useRef<string | null>(null);
  /** A switch warning is already open: don't stack a second one. */
  const warningOpen = React.useRef(false);
  const hinted = React.useRef(new Set<string>());
  /**
   * A CLI sign-in or switch is running (device grant, park, restart). Checks
   * stay quiet until it ends: the daemon's intermediate events (a parked
   * sign-out, the restart) are the app's own doing, never something to follow.
   */
  const cliOp = React.useRef(false);

  const me: SyncMe | null = React.useMemo(() => {
    if (!meData?.id) return null;
    return { id: meData.id, email: meData.email, cloudUrl: platform.apiOrigin ?? client.baseUrl };
  }, [meData, client]);

  /**
   * Merge `patch` into the host record, best-effort. The fresh-sign-in
   * marker survives unless the patch clears it explicitly: an unrelated
   * write (advancing `last_seen_seq` before the session loads) must not
   * eat a sign-in the next check still has to stamp.
   */
  const writeRecord = React.useCallback(
    async (patch: Partial<AuthSyncRecord>): Promise<AuthSyncRecord | null> => {
      if (!runtime) return null;
      const prior = await runtime.getAuthSyncRecord().catch(() => null);
      const next: AuthSyncRecord = { ...(prior ?? {}), ...patch };
      await runtime.setAuthSyncRecord(next).catch(() => undefined);
      return next;
    },
    [runtime]
  );

  /**
   * Act on a switch with runs in flight: ask first (§6.4) and switch only
   * on confirm. `proceed` runs the CLI sign-in (which parks, restores and
   * restarts the daemon). Without a warning the switch is immediate. A
   * cancelled check stays quiet until the risk changes (new runs).
   */
  const askBeforeSwitch = React.useCallback(
    (daemon: DaemonHello, record: AuthSyncRecord | null, me: SyncMe, proceed: () => void) => {
      if (warningOpen.current) return;
      void runtime
        ?.daemonActiveRuns()
        .catch(() => 0)
        .then((active) => {
          if (active <= 0) {
            proceed();
            return;
          }
          const key = `${daemon.auth_event?.seq ?? "seq-less"}:${record?.own_event?.at ?? ""}:${active}`;
          if (deferred.current === key) {
            return;
          }
          warningOpen.current = true;
          const from = daemon.account?.email ?? daemon.account?.user_id ?? "another account";
          const to = me.email ?? me.id;
          setSwitchWarning({
            copy: { from, to, active },
            daemonSeq: daemon.auth_event?.seq ?? null,
            key,
            proceed,
          });
        });
    },
    [runtime]
  );

  const performDecision = React.useCallback(
    async (decision: SyncDecision, hello: DaemonHello, record: AuthSyncRecord | null) => {
      if (!runtime) return;
      const daemonSeq = hello.auth_event?.seq ?? null;
      /** Advance the record after a landed action (no echo, rule 2). */
      const advance = async (seq: number | null): Promise<void> => {
        if (seq !== null) await writeRecord({ last_seen_seq: seq });
      };
      switch (decision.kind) {
        case "none":
          await advance(daemonSeq);
          return;
        case "handoff": {
          try {
            await runtime.desktopSessionHandoff(decision.userId);
          } catch {
            // The daemon cannot hand off (too old, signed out, kill switch
            // off, Windows): fall back to the browser sign-in the user is
            // already looking at, and say so once per account.
            hintFallback(hello, hinted);
            return;
          }
          if (me && me.id !== decision.userId) {
            // §4 row 6: the CLI signed in as someone else, so the app signs
            // its old account out — locally only. No server logout (the
            // handoff already replaced the session cookie) and no CLI logout
            // (the daemon is the side being followed). Every store, the
            // query cache and the runtime identity go, so nothing of the old
            // account shows under the new one.
            const { releaseRuntimeForAccountSwitch } = await import("./service.js");
            await releaseRuntimeForAccountSwitch({ runtime });
            resetRegisteredStores();
            queryClient.clear();
            useSessionStore.getState().markAuthenticated();
          }
          const account = hello.account;
          await writeRecord({
            own_event: {
              kind: "signed_in",
              account: { user_id: account?.user_id, email: account?.email, cloud_url: hello.cloud_url },
              at: nowIso(),
            },
            fresh_sign_in_at: null,
          });
          await advance(daemonSeq);
          // The web session just landed in the cookie store: read me fresh
          // and land in the workspace.
          await queryClient.invalidateQueries({ queryKey: sessionKeys.me });
          void router.navigate({ to: "/" });
          return;
        }
        case "sign_in_cli":
        case "re_sign_in_cli": {
          if (!me) return;
          const cli = platform.cliSetup;
          const run = (): void => {
            // One CLI sign-in at a time, and no check runs while it does
            // (see `cliOp`). Whether it lands or fails, the daemon events it
            // produced are the app's own: mark them seen so none is followed
            // back (no echo, rule 2). A failure leaves the app's sign-in the
            // newest event, so the next check retries the CLI sign-in.
            if (cliOp.current) return;
            cliOp.current = true;
            void (async () => {
              try {
                const { resumeRuntime, signInDaemonForAccount } = await import("./service.js");
                resumeRuntime({ id: me.id, email: me.email, cloudUrl: me.cloudUrl }, { client, runtime });
                await signInDaemonForAccount({
                  client,
                  runtime,
                  cli,
                  force: decision.kind === "sign_in_cli" ? decision.force : undefined,
                });
              } catch {
                // Retried by the next check (see above).
              } finally {
                try {
                  const fresh = await runtime.daemonHello().catch(() => null);
                  await advance(fresh?.auth_event?.seq ?? daemonSeq);
                } finally {
                  cliOp.current = false;
                }
              }
            })().catch(() => undefined);
          };
          if (decision.kind === "sign_in_cli" && decision.switch && hello.account && !sameAccount(me, hello.account)) {
            askBeforeSwitch(hello, record, me, run);
          } else {
            run();
          }
          return;
        }
        case "sign_out_app": {
          notifyInfo(CLI_SIGN_OUT_TITLE, CLI_SIGN_OUT_MESSAGE);
          let landing: string | null = null;
          try {
            landing = await performSignOut({
              client,
              queryClient,
              teardown: platform.sessionTeardown,
              skipRuntimeConfirm: true,
            });
          } catch {
            // A failed sign-out leaves the user where they are; the next
            // check retries.
          }
          await advance(daemonSeq);
          // Land on the signed-out entry, as every other sign-out does.
          if (landing !== null) void router.navigate({ href: landing });
          return;
        }
        case "sign_out_cli": {
          try {
            await runtime.signOutComputer();
          } catch {
            try {
              await runtime.signOutComputer({ localOnly: true });
            } catch {
              // Still failed: the app's sign-out stays the newest event and
              // the next check retries.
            }
          }
          await advance(daemonSeq);
          return;
        }
      }
    },
    [runtime, client, queryClient, router, me, writeRecord, askBeforeSwitch]
  );

  const tick = React.useCallback(async () => {
    if (!runtime || ticking.current || cliOp.current || daemonSetupInProgress()) return;
    ticking.current = true;
    try {
      const hello = await runtime.daemonHello().catch(() => null as DaemonHello | null);
      if (!hello || daemonSetupInProgress()) return;
      // Stamp the app's own sign-in from a fresh marker (never from a
      // restore), once per sign-in.
      let record = await runtime.getAuthSyncRecord().catch(() => null);
      const freshAt = record?.fresh_sign_in_at ?? null;
      if (freshAt !== null && me) {
        const freshMs = Date.parse(freshAt);
        record = await writeRecord({
          own_event:
            !Number.isNaN(freshMs) && Date.now() - freshMs < FRESH_SIGN_IN_WINDOW_MS
              ? { kind: "signed_in", account: { user_id: me.id, email: me.email, cloud_url: me.cloudUrl }, at: freshAt }
              : (record?.own_event ?? null),
          fresh_sign_in_at: null,
        });
      }
      const decision = reconcile({
        record,
        daemon: hello,
        me,
        appCloudUrl: platform.apiOrigin ?? client.baseUrl,
      });
      await performDecision(decision, hello, record);
    } finally {
      ticking.current = false;
    }
  }, [runtime, me, writeRecord, performDecision]);

  // The tick identity follows the session; the timer reads it through the
  // ref so a session refetch does not reset the cadence (or double-tick).
  const tickRef = React.useRef(() => Promise.resolve());
  tickRef.current = () => tick();

  React.useEffect(() => {
    if (!runtime) return;
    let active = true;
    const run = (): void => {
      if (active) void tickRef.current().catch(() => undefined);
    };
    void run();
    const timer = setInterval(run, AUTH_SYNC_INTERVAL_MS);
    const unsubscribe = platform.onFocusChange((focused) => {
      if (focused) run();
    });
    const onOnline = (): void => run();
    globalThis.window.addEventListener("online", onOnline);
    return () => {
      active = false;
      clearInterval(timer);
      unsubscribe();
      globalThis.window.removeEventListener("online", onOnline);
    };
  }, [runtime]);

  // A fresh browser sign-in (marked by the sign-in mutation) also ticks at
  // once, so an app sign-in signs the CLI in without waiting a minute.
  const meId = me?.id ?? null;
  React.useEffect(() => {
    if (meId) void tickRef.current().catch(() => undefined);
  }, [meId]);

  if (!switchWarning) return null;
  return (
    <React.Suspense fallback={null}>
      <LazySwitchWarningDialog
        copy={switchWarning.copy}
        onConfirm={() => {
          const proceed = switchWarning.proceed;
          warningOpen.current = false;
          setSwitchWarning(null);
          proceed();
        }}
        onCancel={() => {
          // Mark the daemon's event seen so the next check does not re-ask
          // (it asks again only when the risk changes: new runs).
          deferred.current = switchWarning.key;
          warningOpen.current = false;
          if (switchWarning.daemonSeq !== null) {
            void writeRecord({ last_seen_seq: switchWarning.daemonSeq });
          }
          setSwitchWarning(null);
        }}
      />
    </React.Suspense>
  );
}

/** Ask for the browser sign-in the handoff fell back to, once per account. */
function hintFallback(hello: DaemonHello, hinted: React.RefObject<Set<string>>): void {
  const account = hello.account;
  if (!account) return;
  const label = account.email ?? account.user_id ?? "this computer";
  const key = `${label}@${hello.daemon_version}`;
  if (hinted.current.has(key)) return;
  hinted.current.add(key);
  notifyInfo(
    `Pi Dash on this computer is signed in as ${label}`,
    "Automatic sign-in is unavailable right now. Sign in with that account and Pi Dash there follows you."
  );
}
