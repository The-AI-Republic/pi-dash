// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval wording and projections (NEWFRONT-553, RUN-033-035): the mode
// vocabulary and its storage key, the availability sentences, the
// narrowest target a request names, and the request's life bound.
import { describe, expect, it } from "vitest";

import {
  APPROVAL_KIND_TEXT,
  APPROVAL_MODES,
  APPROVAL_MODE_COPY,
  APPROVAL_MODE_KEY_PREFIX,
  approvalKindOf,
  approvalModeKey,
  approvalRemainingMs,
  approvalTarget,
  isApprovalMode,
  pendingApprovalFromEvent,
  readApprovalMode,
  rememberApprovalMode,
  storedApprovalMode,
  type PendingApproval,
} from "./approvalCopy.js";

function request(overrides: Partial<PendingApproval> = {}): PendingApproval {
  return {
    localApprovalId: "a-1",
    kind: "command_execution",
    reason: "",
    payload: {},
    expiresAt: null,
    ...overrides,
  };
}

describe("approval modes", () => {
  it("lists the three modes from most to least cautious", () => {
    expect(APPROVAL_MODES).toEqual(["ask", "workspace", "full_access"]);
    for (const mode of APPROVAL_MODES) {
      expect(APPROVAL_MODE_COPY[mode].label).not.toBe("");
      expect(APPROVAL_MODE_COPY[mode].help).not.toBe("");
    }
  });

  it("recognizes only the vocabulary", () => {
    expect(isApprovalMode("ask")).toBe(true);
    expect(isApprovalMode("workspace")).toBe(true);
    expect(isApprovalMode("full_access")).toBe(true);
    expect(isApprovalMode("everything")).toBe(false);
    expect(isApprovalMode(null)).toBe(false);
    expect(isApprovalMode(3)).toBe(false);
  });

  it("reads a stored value and defaults anything else to full access", () => {
    expect(readApprovalMode("ask")).toBe("ask");
    expect(readApprovalMode("workspace")).toBe("workspace");
    expect(readApprovalMode("full_access")).toBe("full_access");
    expect(readApprovalMode("bogus")).toBe("full_access");
    expect(readApprovalMode(null)).toBe("full_access");
    expect(readApprovalMode(undefined)).toBe("full_access");
  });

  it("keeps one storage key per runner under the protocol prefix", () => {
    expect(APPROVAL_MODE_KEY_PREFIX).toBe("pidash:chat-approval-mode:");
    expect(approvalModeKey("r-1")).toBe("pidash:chat-approval-mode:r-1");
  });
});

describe("storedApprovalMode and rememberApprovalMode", () => {
  function storage(initial: Record<string, string> = {}) {
    const entries = new Map(Object.entries(initial));
    return {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => {
        entries.set(key, value);
      },
      entries,
    };
  }

  it("reads back what it remembered for that runner", () => {
    const profile = storage();
    rememberApprovalMode("r-1", "ask", profile);
    expect(profile.entries.get(approvalModeKey("r-1"))).toBe("ask");
    expect(storedApprovalMode("r-1", profile)).toBe("ask");
    expect(storedApprovalMode("r-2", profile)).toBe("full_access");
  });

  it("reads an unavailable profile as the default and survives a refused write", () => {
    const refusing = {
      getItem: () => {
        throw new Error("private mode");
      },
      setItem: () => {
        throw new Error("private mode");
      },
    };
    expect(storedApprovalMode("r-1", refusing)).toBe("full_access");
    expect(() => rememberApprovalMode("r-1", "workspace", refusing)).not.toThrow();
    expect(storedApprovalMode("r-1", undefined)).toBe("full_access");
    expect(() => rememberApprovalMode("r-1", "workspace", undefined)).not.toThrow();
  });
});

describe("approvalKindOf", () => {
  it("maps the contract vocabulary and folds the rest into other", () => {
    expect(approvalKindOf("command_execution")).toBe("command_execution");
    expect(approvalKindOf("file_change")).toBe("file_change");
    expect(approvalKindOf("network_access")).toBe("network_access");
    expect(approvalKindOf("other")).toBe("other");
    expect(approvalKindOf("mystery")).toBe("other");
    expect(approvalKindOf("")).toBe("other");
    for (const kind of ["command_execution", "file_change", "network_access", "other"] as const) {
      expect(APPROVAL_KIND_TEXT[kind]).not.toBe("");
    }
  });
});

describe("approvalTarget", () => {
  it("prefers the most specific thing the request names", () => {
    expect(approvalTarget(request({ payload: { command: "ls", path: "/a", url: "https://x" } }))).toBe("ls");
    expect(approvalTarget(request({ payload: { path: "/a", url: "https://x" } }))).toBe("/a");
    expect(approvalTarget(request({ payload: { url: "https://x", host: "x" } }))).toBe("https://x");
    expect(approvalTarget(request({ payload: { destination: "db", host: "x" } }))).toBe("db");
    expect(approvalTarget(request({ payload: { host: "x" } }))).toBe("x");
  });

  it("reads nothing from an empty or mistyped payload", () => {
    expect(approvalTarget(request())).toBeNull();
    expect(approvalTarget(request({ payload: { command: "", path: 3 } }))).toBeNull();
  });
});

describe("pendingApprovalFromEvent", () => {
  it("projects a translated frame and leaves the target fields in the payload", () => {
    expect(
      pendingApprovalFromEvent({
        local_approval_id: "a-1",
        approval_kind: "command_execution",
        reason: "needs it",
        expires_at: "2026-10-19T10:00:30Z",
        command: "npm test",
        cwd: "/repo",
      })
    ).toEqual({
      localApprovalId: "a-1",
      kind: "command_execution",
      reason: "needs it",
      payload: { command: "npm test", cwd: "/repo" },
      expiresAt: "2026-10-19T10:00:30Z",
    });
  });

  it("supplies empty defaults and folds unknown kinds", () => {
    expect(pendingApprovalFromEvent({ local_approval_id: "a-2", approval_kind: "odd" })).toEqual({
      localApprovalId: "a-2",
      kind: "other",
      reason: "",
      payload: {},
      expiresAt: null,
    });
  });

  it("ignores a reason and a deadline of the wrong type", () => {
    expect(pendingApprovalFromEvent({ local_approval_id: "a-3", reason: 7, expires_at: 12 })).toEqual({
      localApprovalId: "a-3",
      kind: "other",
      reason: "",
      payload: {},
      expiresAt: null,
    });
  });

  it("reads an event without an id as no request", () => {
    expect(pendingApprovalFromEvent({ approval_kind: "command_execution" })).toBeNull();
    expect(pendingApprovalFromEvent({ local_approval_id: 7 })).toBeNull();
  });
});

describe("approvalRemainingMs", () => {
  const now = Date.parse("2026-10-19T10:00:00Z");

  it("answers the time left until the deadline", () => {
    expect(approvalRemainingMs(request({ expiresAt: "2026-10-19T10:00:30Z" }), now)).toBe(30_000);
    expect(approvalRemainingMs(request({ expiresAt: "2026-10-19T09:59:00Z" }), now)).toBe(-60_000);
  });

  it("answers null without a readable deadline", () => {
    expect(approvalRemainingMs(request(), now)).toBeNull();
    expect(approvalRemainingMs(request({ expiresAt: "not a date" }), now)).toBeNull();
  });
});
