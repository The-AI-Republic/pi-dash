// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// On-machine history rules (NEWFRONT-589, DESK-009): the title checks, the
// selection a deletion leaves behind, and the confirmation copy that names
// exactly what is about to be removed.
import { describe, expect, it } from "vitest";

import type { ChatSession } from "@pidash/api-client";

import { makeSession } from "./harness.js";
import {
  CLEAR_HISTORY_TITLE,
  HISTORY_TITLE_MAX,
  HISTORY_TITLE_REQUIRED,
  clearHistoryBody,
  historyTitleError,
  remainingSessions,
  selectionAfterDelete,
} from "./historyActions.js";

describe("historyTitleError", () => {
  it("refuses nothing but whitespace", () => {
    expect(historyTitleError("")).toBe(HISTORY_TITLE_REQUIRED);
    expect(historyTitleError("   ")).toBe(HISTORY_TITLE_REQUIRED);
  });

  it("accepts a name up to the limit and refuses one past it", () => {
    expect(historyTitleError("Planning")).toBeNull();
    expect(historyTitleError("x".repeat(HISTORY_TITLE_MAX))).toBeNull();
    expect(historyTitleError("x".repeat(HISTORY_TITLE_MAX + 1))).toBe(
      `Keep the name under ${HISTORY_TITLE_MAX} characters.`
    );
    // Trimming happens before the length check, so padding does not count.
    expect(historyTitleError(`  ${"x".repeat(HISTORY_TITLE_MAX)}  `)).toBeNull();
  });
});

describe("selectionAfterDelete", () => {
  const older = makeSession({ id: "older", last_message_at: "2026-10-17T10:00:00Z" });
  const newer = makeSession({ id: "newer", last_message_at: "2026-10-18T10:00:00Z" });
  const sessions: ChatSession[] = [older, newer];

  it("keeps the conversation on screen when another one was removed", () => {
    expect(selectionAfterDelete(sessions, "older", "newer")).toBe("newer");
  });

  it("moves to the most recently active survivor when the shown one was removed", () => {
    expect(selectionAfterDelete(sessions, "newer", "newer")).toBe("older");
  });

  it("moves to the most recent survivor when nothing was selected", () => {
    expect(selectionAfterDelete(sessions, "gone", null)).toBe("newer");
  });

  it("answers null when the history is now empty", () => {
    expect(selectionAfterDelete([newer], "newer", "newer")).toBeNull();
  });
});

describe("remainingSessions", () => {
  it("treats an unanswered list as no conversations", () => {
    expect(remainingSessions(undefined)).toEqual([]);
    expect(remainingSessions([])).toEqual([]);
  });

  it("passes the read list through untouched", () => {
    const rows = [makeSession({ id: "s-1" })];
    expect(remainingSessions(rows)).toBe(rows);
  });
});

describe("clearHistoryBody", () => {
  it("names the action with nothing stored instead of a count of zero", () => {
    expect(clearHistoryBody(0)).toMatch(/nothing stored/i);
  });

  it("counts one conversation and many, and always scopes the wipe to this account", () => {
    expect(clearHistoryBody(1)).toContain("All 1 conversation");
    expect(clearHistoryBody(3)).toContain("All 3 conversations");
    expect(clearHistoryBody(3)).toMatch(/other accounts keep theirs/i);
  });
});

describe("dialog titles", () => {
  it("reads as a question the buttons answer", () => {
    expect(CLEAR_HISTORY_TITLE.endsWith("?")).toBe(true);
  });
});
