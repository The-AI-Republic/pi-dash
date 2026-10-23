// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat page rules (NEWFRONT-549, RUN-025/026/028/032): composer gate
// reasons, session selection and fall-through, warm-up decisions, history
// ordering and labels, and the header summary.
import type { ChatSession, Runner } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  buildHistoryEntries,
  chatHeaderSummary,
  CHAT_RUNNER_FALLBACK_NAME,
  COMPOSER_BUSY,
  COMPOSER_CLOSED,
  COMPOSER_LOADING,
  COMPOSER_OFFLINE,
  COMPOSER_REVOKED,
  HISTORY_BUSY_REASON,
  composerBlockReason,
  isExplicitlyViewing,
  isRunnerBusy,
  selectSession,
  servesChat,
  sessionActivityMs,
  sessionSubtitle,
  sessionTitle,
  sortSessionsByActivity,
  warmUpStep,
} from "./chatState.js";

const NOW = Date.parse("2026-10-18T12:00:00Z");

function session(overrides: Partial<ChatSession> = {}): ChatSession {
  return {
    id: "s-1",
    workspace: "ws",
    runner: "r-1",
    runner_detail: { id: "r-1", name: "alpha", status: "online" },
    created_by: "u-1",
    pod: "p-1",
    status: "open",
    agent_kind: "codex",
    local_thread_id: "",
    local_session_id: "",
    cwd: "",
    model: "",
    active_turn_id: "",
    active_message_id: null,
    close_requested: false,
    last_message_at: "2026-10-18T11:00:00Z",
    closed_at: null,
    error: "",
    created_at: "2026-10-18T10:00:00Z",
    updated_at: "2026-10-18T11:00:00Z",
    ...overrides,
  };
}

function runner(overrides: Partial<Runner> = {}): Runner {
  return {
    id: "r-1",
    name: "alpha",
    status: "online",
    host_label: "host",
    provisioning: "manual",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.0.0",
    dev_metadata: {},
    protocol_version: 1,
    capabilities: [],
    last_heartbeat_at: null,
    owner: "u-1",
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: "p-1",
    pod_detail: { id: "p-1", name: "Default" },
    live_state: null,
    enrolled_at: "2026-01-01T00:00:00Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  } as unknown as Runner;
}

describe("composerBlockReason", () => {
  it("blocks until the runner record lands", () => {
    expect(composerBlockReason(null, null)).toBe(COMPOSER_LOADING);
    expect(composerBlockReason(undefined, session())).toBe(COMPOSER_LOADING);
  });

  it("names the runner state that stops chat", () => {
    expect(composerBlockReason(runner({ status: "offline" }), session())).toBe(COMPOSER_OFFLINE);
    expect(composerBlockReason(runner({ status: "revoked" }), session())).toBe(COMPOSER_REVOKED);
    // A row that never enrolled reads offline.
    expect(composerBlockReason(runner({ status: "online", enrolled_at: null }), null)).toBe(COMPOSER_OFFLINE);
  });

  it("blocks a closed session and an in-flight turn", () => {
    expect(composerBlockReason(runner(), session({ status: "closed" }))).toBe(COMPOSER_CLOSED);
    expect(composerBlockReason(runner(), session({ active_message_id: "m-1" }))).toBe(COMPOSER_BUSY);
    expect(composerBlockReason(runner(), session({ active_turn_id: "t-1" }))).toBe(COMPOSER_BUSY);
  });

  it("never blocks a busy runner (busy serves chat) and allows an idle open session", () => {
    expect(composerBlockReason(runner({ status: "busy" }), session())).toBeNull();
    expect(composerBlockReason(runner(), session())).toBeNull();
    expect(composerBlockReason(runner(), null)).toBeNull();
  });
});

describe("isRunnerBusy / servesChat", () => {
  it("reads idle and in-flight sessions", () => {
    expect(isRunnerBusy(null)).toBe(false);
    expect(isRunnerBusy(session())).toBe(false);
    expect(isRunnerBusy(session({ active_turn_id: "t" }))).toBe(true);
  });

  it("admits online and busy runners only", () => {
    expect(servesChat(runner({ status: "online" }))).toBe(true);
    expect(servesChat(runner({ status: "busy" }))).toBe(true);
    expect(servesChat(runner({ status: "offline" }))).toBe(false);
    expect(servesChat(runner({ status: "revoked" }))).toBe(false);
  });
});

describe("history ordering and labels", () => {
  const oldest = session({ id: "oldest", last_message_at: "2024-01-10T10:00:00Z" });
  const middle = session({ id: "middle", last_message_at: "2024-06-10T10:00:00Z" });
  const newest = session({
    id: "newest",
    status: "closed",
    last_message_at: "2024-12-10T10:00:00Z",
    closed_at: "2024-12-11T10:00:00Z",
  });

  it("orders newest activity first and leaves the cached array alone", () => {
    const input = [middle, oldest, newest];
    const sorted = sortSessionsByActivity(input);
    expect(sorted.map((item) => item.id)).toEqual(["newest", "middle", "oldest"]);
    expect(input.map((item) => item.id)).toEqual(["middle", "oldest", "newest"]);
  });

  it("falls back to creation time without activity and treats unparseable stamps as oldest", () => {
    expect(sessionActivityMs({ last_message_at: null, created_at: "2024-03-01T00:00:00Z" })).toBe(
      Date.parse("2024-03-01T00:00:00Z")
    );
    expect(sessionActivityMs({ last_message_at: null, created_at: "nope" })).toBe(0);
    const fresh = session({ id: "fresh", last_message_at: null, created_at: "2024-07-01T00:00:00Z" });
    expect(sortSessionsByActivity([oldest, fresh]).map((item) => item.id)).toEqual(["fresh", "oldest"]);
  });

  it("labels an entry with its activity stamp and a status suffix", () => {
    // The label is localized, so compare against the same formatter.
    const expected = new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(Date.parse("2024-12-10T10:00:00Z"));
    expect(sessionTitle(newest)).toBe(expected);
    expect(sessionTitle(newest)).toContain("Dec 10");
    expect(sessionTitle({ last_message_at: null, created_at: new Date("2024-02-03T15:05:00Z").toISOString() })).toMatch(
      /Feb 03/
    );
    expect(sessionTitle({ last_message_at: null, created_at: "garbage" })).toBe("Chat");
  });

  it("shows a stored conversation's own name instead of its activity stamp", () => {
    expect(sessionTitle({ ...newest, local_title: "Release plan" })).toBe("Release plan");
    // A stored-but-blank name falls back to the stamp like any server row.
    expect(sessionTitle({ ...newest, local_title: "" })).toContain("Dec 10");
    expect(sessionTitle({ ...newest, local_title: "   " })).toContain("Dec 10");
  });

  it("subtitles relative age plus non-open status, or a plain untouched line", () => {
    expect(sessionSubtitle(session({ status: "open" }), NOW)).toBe("1 hour ago");
    expect(sessionSubtitle(session({ status: "failed" }), NOW)).toBe("1 hour ago · failed");
    expect(sessionSubtitle(session({ last_message_at: null }), NOW)).toBe("No turns yet");
    expect(sessionSubtitle(session({ last_message_at: null, status: "closed" }), NOW)).toBe("No turns yet · closed");
  });

  it("marks the active entry and keeps newest-first order", () => {
    const entries = buildHistoryEntries([middle, oldest, newest], "middle", NOW);
    expect(entries.map((entry) => entry.active)).toEqual([false, true, false]);
    expect(entries[0]?.subtitle).toContain("closed");
    expect(entries.map((entry) => entry.id)).toEqual(["newest", "middle", "oldest"]);
    expect(entries.every((entry) => entry.busyReason === null)).toBe(true);
  });

  it("explains why a streaming conversation's actions wait", () => {
    const streaming = { ...middle, active_message_id: "m-1" };
    expect(buildHistoryEntries([streaming], "middle", NOW)[0]?.busyReason).toBe(HISTORY_BUSY_REASON);
  });
});

describe("selectSession", () => {
  const withTurns = session({ id: "with-turns", last_message_at: "2026-10-18T11:00:00Z" });
  const untouched = session({ id: "untouched", last_message_at: null });
  const closed = session({ id: "closed", status: "closed", last_message_at: "2026-10-18T12:00:00Z" });

  it("auto-picks the newest session that has turns, then any open one", () => {
    expect(
      selectSession({ sessions: [untouched, withTurns], pendingId: null, requestedId: null, precreated: null })?.id
    ).toBe("with-turns");
    expect(
      selectSession({ sessions: [untouched, closed], pendingId: null, requestedId: null, precreated: null })?.id
    ).toBe("untouched");
    expect(selectSession({ sessions: [closed], pendingId: null, requestedId: null, precreated: null })).toBeNull();
    expect(selectSession({ sessions: undefined, pendingId: null, requestedId: null, precreated: null })).toBeNull();
  });

  it("picks the most recent open session even when the list order is stale", () => {
    const older = session({ id: "older", last_message_at: "2026-10-01T10:00:00Z" });
    const newer = session({ id: "newer", last_message_at: "2026-10-17T10:00:00Z" });
    expect(selectSession({ sessions: [newer, older], pendingId: null, requestedId: null, precreated: null })?.id).toBe(
      "newer"
    );
    const newerUntouched = session({
      id: "newer-untouched",
      last_message_at: null,
      created_at: "2026-10-18T09:00:00Z",
    });
    expect(
      selectSession({
        sessions: [older, newerUntouched, withTurns],
        pendingId: null,
        requestedId: null,
        precreated: null,
      })?.id
    ).toBe("with-turns");
    const olderUntouched = session({
      id: "older-untouched",
      last_message_at: null,
      created_at: "2026-10-02T09:00:00Z",
    });
    expect(
      selectSession({
        sessions: [olderUntouched, newerUntouched],
        pendingId: null,
        requestedId: null,
        precreated: null,
      })?.id
    ).toBe("newer-untouched");
  });

  it("lets an explicit pending pick beat the address, even for a closed session", () => {
    const picked = selectSession({
      sessions: [withTurns, closed],
      pendingId: "closed",
      requestedId: null,
      precreated: null,
    });
    expect(picked?.id).toBe("closed");
  });

  it("honors a deep link and falls through a stale one", () => {
    expect(
      selectSession({ sessions: [withTurns, closed], pendingId: null, requestedId: "closed", precreated: null })?.id
    ).toBe("closed");
    expect(
      selectSession({ sessions: [withTurns], pendingId: null, requestedId: "missing", precreated: null })?.id
    ).toBe("with-turns");
  });

  it("treats an auto-created session as a fallback, never overriding a pick or a link", () => {
    const created = session({ id: "created", last_message_at: null });
    expect(
      selectSession({ sessions: [withTurns, closed], pendingId: null, requestedId: null, precreated: created })?.id
    ).toBe("with-turns");
    expect(
      selectSession({ sessions: [withTurns, closed], pendingId: null, requestedId: "closed", precreated: created })?.id
    ).toBe("closed");
    expect(
      selectSession({ sessions: [withTurns, closed], pendingId: "closed", requestedId: null, precreated: created })?.id
    ).toBe("closed");
  });

  it("resolves a just-created session before the list catches up", () => {
    const created = session({ id: "new", last_message_at: null });
    expect(selectSession({ sessions: [], pendingId: null, requestedId: null, precreated: created })?.id).toBe("new");
    expect(selectSession({ sessions: [], pendingId: "new", requestedId: null, precreated: created })?.id).toBe("new");
    expect(selectSession({ sessions: [withTurns], pendingId: null, requestedId: "new", precreated: created })?.id).toBe(
      "new"
    );
  });
});

describe("isExplicitlyViewing", () => {
  it("is true only for the named session", () => {
    const picked = session({ id: "picked" });
    expect(isExplicitlyViewing({ session: picked, pendingId: "picked", requestedId: null })).toBe(true);
    expect(isExplicitlyViewing({ session: picked, pendingId: null, requestedId: "picked" })).toBe(true);
    expect(isExplicitlyViewing({ session: picked, pendingId: null, requestedId: "other" })).toBe(false);
    expect(isExplicitlyViewing({ session: null, pendingId: "picked", requestedId: null })).toBe(false);
  });
});

describe("warmUpStep", () => {
  const open = session({ id: "open" });
  const closed = session({ id: "closed", status: "closed" });
  const base = { pendingId: null, requestedId: null, sessionsKnown: true };

  it("warms the open session on screen", () => {
    expect(warmUpStep({ ...base, runner: runner(), session: open })).toEqual({ kind: "warm", sessionId: "open" });
    expect(warmUpStep({ ...base, runner: runner({ status: "busy" }), session: open })).toEqual({
      kind: "warm",
      sessionId: "open",
    });
  });

  it("never warms an unavailable runner", () => {
    expect(warmUpStep({ ...base, runner: null, session: open })).toEqual({ kind: "skip", reason: "no-runner" });
    expect(warmUpStep({ ...base, runner: runner({ status: "offline" }), session: open })).toEqual({
      kind: "skip",
      reason: "runner-cannot-chat",
    });
    expect(warmUpStep({ ...base, runner: runner({ status: "revoked" }), session: null })).toEqual({
      kind: "skip",
      reason: "runner-cannot-chat",
    });
  });

  it("does not replace a history session the viewer picked", () => {
    expect(warmUpStep({ ...base, runner: runner(), session: closed, pendingId: "closed" })).toEqual({
      kind: "skip",
      reason: "viewing-history",
    });
    expect(warmUpStep({ ...base, runner: runner(), session: closed, requestedId: "closed" })).toEqual({
      kind: "skip",
      reason: "viewing-history",
    });
  });

  it("auto-creates once the list is known and nothing is picked", () => {
    expect(warmUpStep({ ...base, runner: runner(), session: null })).toEqual({ kind: "create" });
    expect(warmUpStep({ ...base, runner: runner(), session: null, sessionsKnown: false })).toEqual({
      kind: "skip",
      reason: "sessions-pending",
    });
    // A stale address resolves to no session, so it auto-creates.
    expect(warmUpStep({ ...base, runner: runner(), session: null, requestedId: "stale" })).toEqual({ kind: "create" });
  });
});

describe("chatHeaderSummary", () => {
  it("falls back to a neutral header until the runner lands", () => {
    expect(chatHeaderSummary(null, null)).toEqual({ name: CHAT_RUNNER_FALLBACK_NAME, secondary: "", status: null });
  });

  it("prefers the working directory, then the pod, then the state", () => {
    expect(chatHeaderSummary(runner(), session({ cwd: "/work/tree" })).secondary).toBe("/work/tree");
    expect(chatHeaderSummary(runner(), session({ cwd: "" })).secondary).toBe("Default");
    const unnamedPod = runner();
    delete (unnamedPod as { pod_detail?: unknown }).pod_detail;
    expect(chatHeaderSummary(unnamedPod, null).secondary).toBe("online");
  });

  it("carries the display status, treating a never-enrolled row as offline", () => {
    expect(chatHeaderSummary(runner({ status: "busy" }), null)).toMatchObject({ name: "alpha", status: "busy" });
    expect(chatHeaderSummary(runner({ enrolled_at: null }), null).status).toBe("offline");
  });
});
