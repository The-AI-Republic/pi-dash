// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The reconciliation (desktop_unified_signin §6.2, §9): the newest event
// wins, whichever side made it and whichever account it names. Every
// decision is pinned, including the tricky orderings (seen vs unseen,
// ties, unknown reasons, another cloud).

import { describe, expect, it } from "vitest";

import type { AuthSyncRecord, DaemonHello } from "../../../core/platform/index.js";
import { reconcile, sameAccount, sameCloud, type SyncMe } from "./sync.js";

const CLOUD = "https://cloud.test";
const OTHER_CLOUD = "https://other.test";

const U1: SyncMe = { id: "u1", email: "u1@test", cloudUrl: CLOUD };
const U2: SyncMe = { id: "u2", email: "u2@test", cloudUrl: CLOUD };

function account(id: string, email: string, cloud: string = CLOUD) {
  return { user_id: id, email, cloud_url: cloud };
}

function hello(overrides: Partial<DaemonHello> = {}): DaemonHello {
  return {
    daemon_version: "1.0.0",
    ipc_version: 7,
    cloud_url: CLOUD,
    workspaces: [],
    engine: "available",
    ...overrides,
  };
}

function recordOf(own: AuthSyncRecord["own_event"], lastSeen: number | null = null): AuthSyncRecord {
  return { own_event: own, last_seen_seq: lastSeen };
}

function ownSignedIn(id: string, at: string) {
  return { kind: "signed_in", account: account(id, `${id}@test`), at };
}

function ownSignedOut(id: string, at: string) {
  return { kind: "signed_out", account: account(id, `${id}@test`), at };
}

function daemonEvent(
  kind: "signed_in" | "signed_out",
  seq: number,
  at: string,
  accountOf: string | undefined,
  reason?: string
): NonNullable<DaemonHello["auth_event"]> {
  return {
    seq,
    kind,
    at,
    reason,
    account: accountOf ? account(accountOf, `${accountOf}@test`) : undefined,
  };
}

describe("no daemon, no answer", () => {
  it("skips the comparison and keeps the record", () => {
    expect(reconcile({ record: null, daemon: null, me: null, appCloudUrl: CLOUD })).toEqual({ kind: "none" });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z")),
        daemon: null,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "none" });
  });
});

describe("daemon signed in newest (the app follows, §4, §6.2)", () => {
  it("signs the app in through the handoff when signed out", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 1, "2026-11-16T10:00:00Z", "u1"),
    });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({
      kind: "handoff",
      userId: "u1",
    });
  });

  it("signs the app in as the daemon's account when the app is another account's", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 4, "2026-11-16T10:00:00Z", "u1"),
    });
    expect(
      reconcile({ record: recordOf(ownSignedIn("u2", "2026-11-16T09:00:00Z")), daemon, me: U2, appCloudUrl: CLOUD })
    ).toEqual({ kind: "handoff", userId: "u1" });
  });

  it("does nothing when the app is already that account", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 4, "2026-11-16T10:00:00Z", "u1"),
    });
    expect(
      reconcile({ record: recordOf(ownSignedIn("u1", "2026-11-16T09:00:00Z")), daemon, me: U1, appCloudUrl: CLOUD })
    ).toEqual({ kind: "none" });
  });

  it("ignores a daemon event the app has already seen", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 2, "2026-11-16T10:00:00Z", "u1"),
    });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T09:00:00Z"), 2),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "none" });
  });

  it("cannot hand off without the account's user id", () => {
    const daemon = hello({
      account: { email: "u1@test", cloud_url: CLOUD },
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 1, "2026-11-16T10:00:00Z", undefined),
    });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({ kind: "none" });
  });
});

describe("daemon signed out newest (the app follows, §4, rule 1)", () => {
  it("signs the app out keeping history, whatever account the event names", () => {
    for (const reason of ["logout", "last_workspace_logout", "removed"]) {
      const daemon = hello({
        auth_state: "signed_out",
        auth_event: daemonEvent("signed_out", 3, "2026-11-16T10:00:00Z", "u9", reason),
      });
      expect(
        reconcile({
          record: recordOf(ownSignedIn("u1", "2026-11-16T09:00:00Z")),
          daemon,
          me: U1,
          appCloudUrl: CLOUD,
        }),
        `reason=${reason}`
      ).toEqual({ kind: "sign_out_app" });
    }
  });

  it("does nothing when the app is already signed out", () => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 3, "2026-11-16T10:00:00Z", "u1", "logout"),
    });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({ kind: "none" });
  });

  it("ignores a daemon sign-out older than the app's own sign-in", () => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 3, "2026-11-16T08:00:00Z", "u1", "logout"),
    });
    // Own sign-in is newer: the CLI follows, back to signed in.
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z")),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_in_cli", force: false, switch: false });
  });
});

describe("expired or revoked daemon credentials never win (rule 1)", () => {
  it.each([["expired"], ["revoked"]])("re-signs the CLI in when the app is signed in, reason=%s", (reason) => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 5, "2026-11-16T10:00:00Z", "u1", reason),
    });
    expect(
      reconcile({ record: recordOf(ownSignedIn("u1", "2026-11-16T09:00:00Z")), daemon, me: U1, appCloudUrl: CLOUD })
    ).toEqual({ kind: "re_sign_in_cli" });
  });

  it.each([["expired"], ["revoked"]])("does nothing when the app is signed out, reason=%s", (reason) => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 5, "2026-11-16T10:00:00Z", "u1", reason),
    });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({ kind: "none" });
  });
});

describe("app newest (the CLI follows)", () => {
  it("signs the CLI in when the daemon is signed out", () => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 2, "2026-11-16T09:00:00Z", "u1", "logout"),
    });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z"), 2),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_in_cli", force: false, switch: false });
  });

  it("switches the CLI when the daemon is another account", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 3, "2026-11-16T08:00:00Z", "u1"),
    });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u2", "2026-11-16T10:00:00Z"), 3),
        daemon,
        me: U2,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_in_cli", force: false, switch: true });
  });

  it("retries the CLI logout when the app's sign-out never landed (rule 2)", () => {
    const daemon = hello({
      account: account("u1", "u1@test"),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 3, "2026-11-16T09:00:00Z", "u1"),
    });
    expect(
      reconcile({
        record: recordOf(ownSignedOut("u1", "2026-11-16T10:00:00Z"), 3),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_out_cli" });
  });

  it("ignores a tie: the daemon wins", () => {
    const at = "2026-11-16T10:00:00Z";
    const daemon = hello({ auth_state: "signed_out", auth_event: daemonEvent("signed_out", 2, at, "u1", "logout") });
    expect(reconcile({ record: recordOf(ownSignedIn("u1", at)), daemon, me: U1, appCloudUrl: CLOUD })).toEqual({
      kind: "sign_out_app",
    });
  });

  it("treats an unreadable time as the daemon's", () => {
    const daemon = hello({
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 2, "not-a-time", "u1", "logout"),
    });
    expect(
      reconcile({ record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z")), daemon, me: U1, appCloudUrl: CLOUD })
    ).toEqual({ kind: "sign_out_app" });
  });
});

describe("another cloud (§6.2)", () => {
  it("switches a daemon on another cloud to the app's cloud", () => {
    const daemon = hello({
      cloud_url: OTHER_CLOUD,
      auth_state: "signed_out",
      auth_event: daemonEvent("signed_out", 2, "2026-11-16T09:00:00Z", "u1", "logout"),
    });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z"), 2),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_in_cli", force: true, switch: false });
  });

  it("does not follow a daemon signed in to another cloud", () => {
    const daemon = hello({
      cloud_url: OTHER_CLOUD,
      account: account("u1", "u1@test", OTHER_CLOUD),
      auth_state: "signed_in",
      auth_event: daemonEvent("signed_in", 2, "2026-11-16T10:00:00Z", "u1"),
    });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({ kind: "none" });
  });
});

describe("daemons from before auth_event (§8)", () => {
  it("signs the app in with no record of its own", () => {
    const daemon = hello({ account: account("u1", "u1@test"), auth_state: "signed_in" });
    expect(reconcile({ record: null, daemon, me: null, appCloudUrl: CLOUD })).toEqual({
      kind: "handoff",
      userId: "u1",
    });
  });

  it("signs the app out with no record of its own", () => {
    const daemon = hello({ auth_state: "signed_out" });
    expect(reconcile({ record: null, daemon, me: U1, appCloudUrl: CLOUD })).toEqual({ kind: "sign_out_app" });
  });

  it("converges a fresh app sign-in with a signed-out old daemon", () => {
    const daemon = hello({ auth_state: "signed_out" });
    expect(
      reconcile({
        record: recordOf(ownSignedIn("u1", "2026-11-16T10:00:00Z")),
        daemon,
        me: U1,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "sign_in_cli", force: false, switch: false });
  });

  it("signs back in with the daemon's account after the app signed out", () => {
    const daemon = hello({ account: account("u1", "u1@test"), auth_state: "signed_in" });
    expect(
      reconcile({
        record: recordOf(ownSignedOut("u1", "2026-11-16T10:00:00Z")),
        daemon,
        me: null,
        appCloudUrl: CLOUD,
      })
    ).toEqual({ kind: "handoff", userId: "u1" });
  });
});

describe("sameAccount / sameCloud", () => {
  it("decides by user id, then e-mail, then nothing", () => {
    expect(sameAccount(U1, account("u1", "other@test"))).toBe(true);
    expect(sameAccount(U1, account("u2", "u1@test"))).toBe(false);
    expect(sameAccount({ ...U1, id: "" }, { email: "U1@Test" })).toBe(true);
    expect(sameAccount(null, account("u1", "u1@test"))).toBe(false);
    expect(sameAccount(U1, undefined)).toBe(false);
    expect(sameAccount(U1, {})).toBe(true);
  });

  it("compares scheme, host and port", () => {
    expect(sameCloud(CLOUD, "https://cloud.test/")).toBe(true);
    expect(sameCloud("https://cloud.test:8443", CLOUD)).toBe(false);
    expect(sameCloud("http://cloud.test", CLOUD)).toBe(false);
  });
});
