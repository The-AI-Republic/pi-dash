// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs presentation helpers (NEWFRONT-545). Page parsing, executor labels,
// diagnostics fallbacks, transcript narration, result summary and time
// formatting each keep one tested definition.
import type { AgentRun, RunErrorDiagnostic, RunEvent } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  errorKindLabel,
  errorSourceTone,
  eventNarrative,
  eventTime,
  executorLabel,
  executorTone,
  hasFailure,
  pageLabel,
  parseRunsPage,
  resultJson,
  resultSummary,
  runsPageSearch,
  startedLabel,
} from "./runsModel.js";

const run = (overrides: Partial<AgentRun> = {}): AgentRun =>
  ({
    executor_kind: "local_runner",
    done_payload: null,
    error: "",
    started_at: "2024-01-02T03:04:05Z",
    created_at: "2024-01-01T00:00:00Z",
    ...overrides,
  }) as AgentRun;

const event = (overrides: Partial<RunEvent> = {}): RunEvent =>
  ({ id: 1, seq: 1, kind: "run/started", payload: {}, created_at: "2024-01-01T00:00:00Z", ...overrides }) as RunEvent;

describe("parseRunsPage", () => {
  it("reads positive integers from strings and numbers", () => {
    expect(parseRunsPage({ page: "2" })).toBe(2);
    expect(parseRunsPage({ page: 3 })).toBe(3);
    expect(parseRunsPage({ page: "4.9" })).toBe(4);
  });

  it("falls back to page one for absent, malformed and non-positive values", () => {
    expect(parseRunsPage({})).toBe(1);
    expect(parseRunsPage({ page: "abc" })).toBe(1);
    expect(parseRunsPage({ page: 0 })).toBe(1);
    expect(parseRunsPage({ page: -3 })).toBe(1);
    expect(parseRunsPage({ page: {} })).toBe(1);
  });
});

describe("runsPageSearch", () => {
  it("omits page one and keeps later pages", () => {
    expect(runsPageSearch(1)).toEqual({});
    expect(runsPageSearch(0)).toEqual({});
    expect(runsPageSearch(2)).toEqual({ page: 2 });
  });

  it("labels the footer", () => {
    expect(pageLabel(2, 5)).toBe("Page 2 of 5");
  });
});

describe("executor presentation", () => {
  it("separates the cloud agent from every local executor", () => {
    expect(executorLabel("cloud_agent")).toBe("Pi Dash Cloud Agent");
    expect(executorLabel("local_runner")).toBe("Local Runner");
    expect(executorLabel("managed_runner")).toBe("Local Runner");
    expect(executorTone("cloud_agent")).toBe("accent");
    expect(executorTone("managed_runner")).toBe("neutral");
  });
});

describe("failure helpers", () => {
  it("treats only non-empty error text as a failure", () => {
    expect(hasFailure(run({ error: "boom" }))).toBe(true);
    expect(hasFailure(run())).toBe(false);
  });

  it("maps known diagnostic kinds and keeps unknown tokens", () => {
    expect(errorKindLabel("agent_authentication")).toBe("Authentication");
    expect(errorKindLabel("agent_model_access")).toBe("Model access");
    expect(errorKindLabel("runner_registration")).toBe("Runner registration");
    expect(errorKindLabel("runner_lifecycle")).toBe("Runner lifecycle");
    expect(errorKindLabel("agent_stalled")).toBe("Agent stalled");
    expect(errorKindLabel("unknown")).toBe("Unknown");
    expect(errorKindLabel("future_kind")).toBe("future_kind");
  });

  it("tones every error source", () => {
    const sources: RunErrorDiagnostic["source"][] = ["agent", "pidash_runner", "pidash_cloud", "unknown"];
    expect(sources.map(errorSourceTone)).toEqual(["warning", "accent", "accent", "neutral"]);
  });
});

describe("eventNarrative", () => {
  it("expands non-blank agent messages", () => {
    expect(eventNarrative(event({ kind: "agent/message", payload: { text: "hello" } }))).toBe("hello");
  });

  it("hides other kinds, blank text and non-string text", () => {
    expect(eventNarrative(event({ kind: "run/started", payload: { text: "hello" } }))).toBeNull();
    expect(eventNarrative(event({ kind: "agent/message", payload: { text: "   " } }))).toBeNull();
    expect(eventNarrative(event({ kind: "agent/message", payload: { text: 42 } }))).toBeNull();
  });
});

describe("result helpers", () => {
  it("surfaces the cloud summary only for cloud runs", () => {
    expect(resultSummary(run({ executor_kind: "cloud_agent", done_payload: { summary: "done" } }))).toBe("done");
    expect(resultSummary(run({ executor_kind: "local_runner", done_payload: { summary: "done" } }))).toBeNull();
    expect(resultSummary(run({ executor_kind: "cloud_agent", done_payload: { summary: " " } }))).toBeNull();
    expect(resultSummary(run({ executor_kind: "cloud_agent", done_payload: null }))).toBeNull();
    expect(resultSummary(run({ executor_kind: "cloud_agent", done_payload: { summary: 3 } }))).toBeNull();
  });

  it("serializes the raw payload with indentation", () => {
    expect(resultJson({ answer: 42 })).toBe('{\n  "answer": 42\n}');
  });
});

describe("time helpers", () => {
  it("prefers the started time and falls back to creation", () => {
    expect(startedLabel(run())).toBe(new Date("2024-01-02T03:04:05Z").toLocaleString());
    expect(startedLabel(run({ started_at: null }))).toBe(new Date("2024-01-01T00:00:00Z").toLocaleString());
  });

  it("formats event clock times", () => {
    expect(eventTime("2024-01-01T00:00:00Z")).toBe(new Date("2024-01-01T00:00:00Z").toLocaleTimeString());
  });
});
