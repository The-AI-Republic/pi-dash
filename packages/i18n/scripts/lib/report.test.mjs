import { describe, expect, it, vi } from "vitest";

import {
  buildReportEntry,
  groupIssuesByLocale,
  normalizeIssues,
  pendingEvaluations,
  resolveCorrection,
} from "./report.mjs";

describe("normalizeIssues", () => {
  it("returns a bare array unchanged", () => {
    const arr = [{ key: "a" }];
    expect(normalizeIssues(arr)).toBe(arr);
  });

  it("unwraps a { issues: [...] } envelope", () => {
    const issues = [{ key: "a" }];
    expect(normalizeIssues({ issues })).toBe(issues);
  });

  it("returns [] for null / non-conforming shapes", () => {
    expect(normalizeIssues(null)).toEqual([]);
    expect(normalizeIssues({})).toEqual([]);
    expect(normalizeIssues({ issues: "nope" })).toEqual([]);
    expect(normalizeIssues(42)).toEqual([]);
  });
});

describe("groupIssuesByLocale", () => {
  it("buckets valid issues by locale", () => {
    const grouped = groupIssuesByLocale([
      { locale: "fr", key: "Hello" },
      { locale: "fr", key: "Bye" },
      { locale: "ja", key: "Hello" },
    ]);
    expect(grouped.get("fr")).toHaveLength(2);
    expect(grouped.get("ja")).toHaveLength(1);
  });

  it("drops the English fallback and unknown locales", () => {
    const grouped = groupIssuesByLocale([
      { locale: "en", key: "Hello" },
      { locale: "xx", key: "Hello" },
      { locale: "fr", key: "Hello" },
    ]);
    expect(grouped.has("en")).toBe(false);
    expect(grouped.has("xx")).toBe(false);
    expect(grouped.get("fr")).toHaveLength(1);
  });

  it("drops malformed entries (missing locale or key)", () => {
    const grouped = groupIssuesByLocale([
      null,
      { key: "Hello" },
      { locale: "fr" },
      { locale: "fr", key: 5 },
      { locale: "fr", key: "Hello" },
    ]);
    expect(grouped.get("fr")).toHaveLength(1);
  });
});

describe("buildReportEntry", () => {
  const item = { key: "Hello", source: "Hello", current: "Bonjour" };

  it("keeps a real, placeholder-safe suggestion", () => {
    const entry = buildReportEntry("fr", item, { reason: "wrong", suggestion: "Salut" });
    expect(entry).toEqual({ locale: "fr", key: "Hello", current: "Bonjour", reason: "wrong", suggestion: "Salut" });
  });

  it("blanks a suggestion identical to the current value", () => {
    const entry = buildReportEntry("fr", item, { reason: "x", suggestion: "Bonjour" });
    expect(entry.suggestion).toBe("");
  });

  it("blanks an empty / missing suggestion but preserves the flag + reason", () => {
    expect(buildReportEntry("fr", item, { reason: "bad" }).suggestion).toBe("");
    expect(buildReportEntry("fr", item, { reason: "bad", suggestion: "   " }).suggestion).toBe("");
    expect(buildReportEntry("fr", item, { reason: "bad" }).reason).toBe("bad");
  });

  it("blanks a suggestion that drops an ICU placeholder", () => {
    const placeholderItem = { key: "Hi {name}", source: "Hi {name}", current: "Salut {name}" };
    const entry = buildReportEntry("fr", placeholderItem, { reason: "x", suggestion: "Salut toi" });
    expect(entry.suggestion).toBe("");
  });

  it("keeps a suggestion that preserves the ICU placeholder", () => {
    const placeholderItem = { key: "Hi {name}", source: "Hi {name}", current: "Salut {name}" };
    const entry = buildReportEntry("fr", placeholderItem, { reason: "x", suggestion: "Bonjour {name}" });
    expect(entry.suggestion).toBe("Bonjour {name}");
  });
});

describe("pendingEvaluations", () => {
  it("includes flagged keys whose current value still matches the report", () => {
    const issues = [{ locale: "fr", key: "Hello", current: "Bonjour", suggestion: "Salut", reason: "x" }];
    const items = pendingEvaluations(issues, { Hello: "Bonjour" });
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ key: "Hello", source: "Hello", current: "Bonjour", suggestion: "Salut" });
    expect(items[0].id).toBe("issue_1");
  });

  it("skips keys missing or empty in the current file", () => {
    const issues = [
      { locale: "fr", key: "Gone", current: "X" },
      { locale: "fr", key: "Empty", current: "X" },
    ];
    const items = pendingEvaluations(issues, { Empty: "" });
    expect(items).toHaveLength(0);
  });

  it("skips and reports keys edited since the report was generated", () => {
    const onSkip = vi.fn();
    const issues = [{ locale: "fr", key: "Hello", current: "Bonjour" }];
    const items = pendingEvaluations(issues, { Hello: "Coucou" }, onSkip);
    expect(items).toHaveLength(0);
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it("applies when the report did not record a prior value (no drift guard)", () => {
    const issues = [{ locale: "fr", key: "Hello" }];
    const items = pendingEvaluations(issues, { Hello: "Bonjour" });
    expect(items).toHaveLength(1);
    expect(items[0].current).toBe("Bonjour");
  });
});

describe("resolveCorrection", () => {
  const item = { key: "Hello", source: "Hello", current: "Bonjour" };

  it("returns the final string for an 'incorrect' verdict with a safe replacement", () => {
    expect(resolveCorrection({ verdict: "incorrect", final: "Salut" }, item, "fr")).toBe("Salut");
  });

  it("returns null for an 'acceptable' verdict (keeps current)", () => {
    expect(resolveCorrection({ verdict: "acceptable", final: "Salut" }, item, "fr")).toBeNull();
  });

  it("returns null when the verdict is missing or malformed", () => {
    expect(resolveCorrection(null, item, "fr")).toBeNull();
    expect(resolveCorrection({}, item, "fr")).toBeNull();
    expect(resolveCorrection({ verdict: "incorrect" }, item, "fr")).toBeNull();
  });

  it("returns null when the correction is empty or unchanged", () => {
    expect(resolveCorrection({ verdict: "incorrect", final: "   " }, item, "fr")).toBeNull();
    expect(resolveCorrection({ verdict: "incorrect", final: "Bonjour" }, item, "fr")).toBeNull();
  });

  it("rejects a correction that breaks an ICU placeholder", () => {
    const placeholderItem = { key: "Hi {name}", source: "Hi {name}", current: "Salut {name}" };
    expect(resolveCorrection({ verdict: "incorrect", final: "Salut toi" }, placeholderItem, "fr")).toBeNull();
    expect(resolveCorrection({ verdict: "incorrect", final: "Bonjour {name}" }, placeholderItem, "fr")).toBe(
      "Bonjour {name}"
    );
  });
});
