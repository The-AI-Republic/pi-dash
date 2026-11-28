// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Skipped tool-server notices (AGT-046): the replay key, the anchor rule,
// and every reason phrasing the oracle pins plus the raw fallback.
import { afterEach, describe, expect, it } from "vitest";
import type { AssistantMessage } from "@pidash/api-client";

import { edition } from "../../../core/edition/index.js";
import { isOpenhubFreeQuotaEnabled } from "../../../core/edition/index.js";
import { formatResetsAt, noticeAnchorSeq, noticeId, skippedServerLine, turnFailureNotice } from "./notices.js";

function row(seq: number): AssistantMessage {
  return {
    id: `m${seq}`,
    role: "user",
    content: "ask",
    status: "completed",
    seq,
    turn_id: null,
    payload: {},
    created_at: "2026-10-01T00:00:00Z",
    completed_at: "2026-10-01T00:00:00Z",
  };
}

describe("noticeId", () => {
  it("keys one notice per skipped-servers event", () => {
    expect(noticeId(701)).toBe("notice:701");
  });
});

describe("noticeAnchorSeq", () => {
  it("anchors after the highest row sequence on screen", () => {
    expect(noticeAnchorSeq({ a: row(1), b: row(4), c: row(2) })).toBe(4);
  });

  it("anchors at zero on an empty transcript", () => {
    expect(noticeAnchorSeq({})).toBe(0);
  });
});

describe("skippedServerLine", () => {
  it("maps a blocked URL to plain language", () => {
    expect(skippedServerLine({ name: "closed", reason: "url_blocked" })).toBe(
      "Tool server closed was unavailable for this reply (its URL is not allowed)."
    );
  });

  it("maps an unreachable toolset to plain language", () => {
    expect(skippedServerLine({ name: "poll-notice", reason: "toolset_unavailable" })).toBe(
      "Tool server poll-notice was unavailable for this reply (it could not be reached)."
    );
  });

  it("maps an unreadable credential to plain language", () => {
    expect(skippedServerLine({ name: "vault", reason: "auth_header_unreadable" })).toBe(
      "Tool server vault was unavailable for this reply (its credential could not be read)."
    );
  });

  it("maps an undecryptable credential to plain language", () => {
    expect(skippedServerLine({ name: "vault", reason: "assistant_not_configured" })).toBe(
      "Tool server vault was unavailable for this reply (its credential could not be decrypted)."
    );
  });

  it("maps an OpenHub outage to plain language", () => {
    expect(skippedServerLine({ name: "hub", reason: "openhub_unavailable" })).toBe(
      "Tool server hub was unavailable for this reply (OpenHub apps were unavailable)."
    );
  });

  it("phrases a whole outage as one capability line", () => {
    expect(skippedServerLine({ name: "all tool servers", reason: "toolsets_unavailable" })).toBe(
      "Tool servers were unavailable for this reply (they could not be loaded)."
    );
  });

  it("renders an unmapped reason as it arrived", () => {
    expect(skippedServerLine({ name: "dead-a", reason: "RuntimeError" })).toBe(
      "Tool server dead-a was unavailable for this reply (RuntimeError)."
    );
  });
});

describe("isOpenhubFreeQuotaEnabled", () => {
  const originalId = edition.id;
  const originalFlags = { ...edition.flags };
  afterEach(() => {
    edition.id = originalId;
    edition.flags = { ...originalFlags };
  });

  it("is off on OSS and on when the cloud edition id is set", () => {
    const saved = { ...edition.flags };
    // Restore any mutation from a previous test.
    delete edition.flags["openhub-free-quota"];
    edition.id = "oss";
    expect(isOpenhubFreeQuotaEnabled(edition)).toBe(false);
    edition.id = "cloud";
    expect(isOpenhubFreeQuotaEnabled(edition)).toBe(true);
    edition.flags = { ...saved };
  });

  it("is on when the flag is set even on OSS id", () => {
    edition.id = "oss";
    edition.flags = { ...edition.flags, "openhub-free-quota": true };
    expect(isOpenhubFreeQuotaEnabled(edition)).toBe(true);
  });
});

describe("formatResetsAt", () => {
  it("formats a UTC ISO in the local zone", () => {
    const iso = "2026-08-15T00:00:00Z";
    const out = formatResetsAt(iso);
    // The exact text depends on the test runner zone, but it must be a
    // localized date/time, not the raw ISO, and must contain a year.
    expect(out).not.toBe(iso);
    expect(out).toMatch(/2026/);
    expect(out.length).toBeGreaterThan(iso.length);
  });

  it("returns the raw value when it does not parse", () => {
    expect(formatResetsAt("not-a-date")).toBe("not-a-date");
  });

  it("returns empty for empty", () => {
    expect(formatResetsAt("")).toBe("");
    expect(formatResetsAt("   ")).toBe("");
  });
});

describe("turnFailureNotice (NEWPIDASH-148, cloud vs OSS)", () => {
  const originalId = edition.id;
  const originalFlags = { ...edition.flags };

  // Drive the cloud gate by id without touching the private overlay.
  function setCloud() {
    edition.id = "cloud";
    edition.flags = { ...edition.flags };
  }
  function setOss() {
    edition.id = "oss";
    edition.flags = { ...edition.flags };
    delete edition.flags["openhub-free-quota"];
  }

  afterEach(() => {
    edition.id = originalId;
    edition.flags = { ...originalFlags };
  });

  it("OSS stays verbatim for every new code", () => {
    setOss();
    // OSS must never map, even for the new codes — the backend never sends
    // them on OSS, but the gate guarantees it.
    expect(
      turnFailureNotice({
        error_code: "openhub_free_quota_exhausted",
        detail: "Daily free limit reached, resets at 2026-08-15T00:00:00Z.",
      })
    ).toBe("Daily free limit reached, resets at 2026-08-15T00:00:00Z.");
    expect(turnFailureNotice({ error_code: "openhub_paid_model_needs_credit", detail: "backend detail" })).toBe(
      "backend detail"
    );
    expect(turnFailureNotice({ error_code: "openhub_wallet_empty", detail: "Top up to keep using." })).toBe(
      "Top up to keep using."
    );
    expect(turnFailureNotice({ error_code: "openhub_free_lane_busy", detail: "busy" })).toBe("busy");
    expect(turnFailureNotice({ error_code: "openhub_free_quota_unavailable", detail: "unavailable" })).toBe(
      "unavailable"
    );
  });

  it("maps free_quota_exhausted to its own notice with localized reset time", () => {
    setCloud();
    const iso = "2026-08-15T00:00:00Z";
    const out = turnFailureNotice({
      error_code: "openhub_free_quota_exhausted",
      detail: "Daily free limit reached, resets at 2026-08-15T00:00:00Z.",
      resets_at: iso,
    });
    expect(out.startsWith("Daily free limit reached, resets at ")).toBe(true);
    // Structured resets_at was re-formatted locally, not echoed raw.
    expect(out).not.toBe("Daily free limit reached, resets at 2026-08-15T00:00:00Z.");
    expect(out).toMatch(/2026/);
  });

  it("keeps the backend detail when no structured resets_at is present", () => {
    setCloud();
    const backendDetail = "Daily free limit reached, resets at August 14, 2026 at 07:00 PM CDT.";
    expect(turnFailureNotice({ error_code: "openhub_free_quota_exhausted", detail: backendDetail })).toBe(
      backendDetail
    );
  });

  it("maps paid_model_needs_credit to the needs-credit notice", () => {
    setCloud();
    expect(
      turnFailureNotice({
        error_code: "openhub_paid_model_needs_credit",
        detail: "This model needs credit. The OpenHub free model is included on every plan.",
      })
    ).toBe("This model needs credit. The OpenHub free model is included on every plan.");
  });

  it("keeps the wallet-empty top-up notice", () => {
    setCloud();
    const wallet = "Your OpenHub wallet is out of credits. Top up to keep using the assistant.";
    expect(turnFailureNotice({ error_code: "openhub_wallet_empty", detail: wallet })).toBe(wallet);
  });

  it("maps free_lane_busy (429) to the retryable busy notice", () => {
    setCloud();
    const busy = "Too many free requests at once. Try again in a moment.";
    expect(turnFailureNotice({ error_code: "openhub_free_lane_busy", detail: busy })).toBe(busy);
    expect(turnFailureNotice({ error_code: "openhub_free_lane_busy", detail: "" })).toBe(busy);
  });

  it("maps free_quota_unavailable (503) to the retryable unavailable notice", () => {
    setCloud();
    const unavail = "Free quota is unavailable right now. Try again shortly.";
    expect(turnFailureNotice({ error_code: "openhub_free_quota_unavailable", detail: unavail })).toBe(unavail);
    expect(turnFailureNotice({ error_code: "openhub_free_quota_unavailable", detail: "" })).toBe(unavail);
  });

  it("falls back to verbatim for an unknown code", () => {
    setCloud();
    expect(turnFailureNotice({ error_code: "provider_unreachable", detail: "Could not reach." })).toBe(
      "Could not reach."
    );
    expect(turnFailureNotice({ error_code: "provider_error", detail: "" })).toBe("provider_error");
  });
});
