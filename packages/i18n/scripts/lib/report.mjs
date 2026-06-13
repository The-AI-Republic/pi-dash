// Pure helpers for the validation-report contract shared between
// validate-translations (producer) and translate-missing (consumer). No file
// IO or network here so the decision logic stays unit-testable.

import { fallbackLanguage, hasMatchingIcuArguments, isValidIcu, languageByValue } from "./shared.mjs";

// Coerce an LLM/file response into a plain array of issue-ish objects.
// Accepts either a bare array or a { issues: [...] } envelope.
export function normalizeIssues(parsed) {
  if (Array.isArray(parsed)) return parsed;
  if (parsed && Array.isArray(parsed.issues)) return parsed.issues;
  return [];
}

// Group report issues by target locale, dropping malformed entries and any
// locale we don't translate into (unknown or the English fallback).
export function groupIssuesByLocale(issues) {
  const byLocale = new Map();
  for (const issue of issues) {
    if (!issue || typeof issue.locale !== "string" || typeof issue.key !== "string") continue;
    if (!languageByValue.has(issue.locale) || issue.locale === fallbackLanguage) continue;
    const bucket = byLocale.get(issue.locale) || [];
    bucket.push(issue);
    byLocale.set(issue.locale, bucket);
  }
  return byLocale;
}

// Turn one reviewer issue into a normalized report entry. `suggestion` is kept
// only when it is a real, placeholder-safe change; otherwise it is blanked so
// the downstream apply pass treats it as "no proposal".
export function buildReportEntry(locale, item, raw) {
  const suggestion = typeof raw.suggestion === "string" ? raw.suggestion.trim() : "";
  const reason = typeof raw.reason === "string" ? raw.reason.trim() : "";
  const suggestionSafe =
    suggestion.length > 0 &&
    suggestion !== item.current &&
    isValidIcu(suggestion, locale) &&
    hasMatchingIcuArguments(item.source, suggestion, locale);

  return {
    locale,
    key: item.key,
    current: item.current,
    reason,
    suggestion: suggestionSafe ? suggestion : "",
  };
}

// Build the apply work-list for one locale: keep only flagged keys that still
// exist with a non-empty value matching what the report recorded. A translation
// edited since the report was generated is skipped (not silently reverted).
export function pendingEvaluations(issues, currentTranslations, onSkip) {
  const items = [];
  for (const [index, issue] of issues.entries()) {
    const current = currentTranslations[issue.key];
    if (typeof current !== "string" || current.length === 0) continue;
    if (typeof issue.current === "string" && issue.current.length > 0 && issue.current !== current) {
      onSkip?.(issue);
      continue;
    }
    items.push({
      id: `issue_${index + 1}`,
      key: issue.key,
      source: issue.key,
      current,
      suggestion: typeof issue.suggestion === "string" ? issue.suggestion : "",
      reason: typeof issue.reason === "string" ? issue.reason : "",
    });
  }
  return items;
}

// Decide whether an LLM evaluation verdict yields a safe replacement to write.
// Returns the final string to apply, or null to keep the current translation.
export function resolveCorrection(verdict, item, locale) {
  if (!verdict || verdict.verdict !== "incorrect") return null;
  const final = typeof verdict.final === "string" ? verdict.final.trim() : "";
  if (final.length === 0 || final === item.current) return null;
  if (!isValidIcu(final, locale) || !hasMatchingIcuArguments(item.source, final, locale)) return null;
  return final;
}
