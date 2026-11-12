// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API smoke test for the prompts feature (NEWFRONT-599): other
// features and routes import only from this barrel, so every binding the
// receipt slice adds must resolve at runtime.
import { describe, expect, it } from "vitest";

import * as prompts from "./index.js";

describe("prompts feature public API", () => {
  it("exposes the receipt, preview, tab and state bindings", () => {
    expect(prompts.PROMPT_TABS).toEqual(["sections", "receipt"]);
    expect(prompts.PromptsTabs).toBeTypeOf("function");
    expect(prompts.PromptsView).toBeTypeOf("function");
    expect(prompts.ReceiptView).toBeTypeOf("function");
    expect(prompts.ReceiptCard).toBeTypeOf("function");
    expect(prompts.PreviewPanel).toBeTypeOf("function");
    expect(prompts.RECEIPTS_LOADING_TEXT).toBe("Loading…");
    expect(prompts.RECEIPTS_FAILED_TEXT).toContain("Could not load prompt receipts");
    expect(prompts.AUTOMATIC_RUNS_LABEL).toContain("Automatic runs");
  });

  it("exposes the receipt data layer and helpers", () => {
    expect(prompts.promptReceiptKeys.compiled("acme", "review")).toEqual(
      expect.arrayContaining(["ws", "acme", "prompts", "receipts", "review"])
    );
    expect(prompts.compiledPromptQueryOptions).toBeTypeOf("function");
    expect(prompts.usePromptReceipts).toBeTypeOf("function");
    expect(prompts.usePreviewPrompt).toBeTypeOf("function");
    expect(prompts.previewFailureMessage(new Error("nope"))).toBe("nope");
    expect(prompts.sectionCountLabel(2)).toBe("2 sections");
    expect(prompts.receiptAnchorId("scheduler")).toBe("prompt-receipt-scheduler");
    expect(prompts.receiptSlots([])).toEqual([]);
    expect(prompts.automaticTemplate({ kind: "review", scope: "user", template_body: "x" })).toBeNull();
    expect(prompts.previewTargetMode("scheduler")).toBe("install");
    expect(prompts.previewTargetLabel("review")).toBe("Issue id");
    expect(prompts.previewTargetPlaceholder("scheduler")).toContain("Scheduler install id");
    expect(prompts.previewTargetPayload("review", "i")).toEqual({ issue_id: "i" });
    expect(prompts.draftPreviewKinds(["review"])).toEqual([]);
    expect(prompts.initialDraftKind([])).toBe("coding-task");
    expect(prompts.MISSING_ISSUE_TARGET).toContain("issue");
    expect(prompts.MISSING_INSTALL_TARGET).toContain("install");
    expect(prompts.PREVIEW_FAILED_FALLBACK).toContain("could not be rendered");
  });
});
