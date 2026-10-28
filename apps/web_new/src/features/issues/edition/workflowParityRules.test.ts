// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Development-only workflow rules (ISS-233): the provider refuses
// exactly the injected hops, reads a missing or broken key as no rules,
// and never throws on storage or parse failures.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  PARITY_WORKFLOW_RULES_KEY,
  parityWorkflowRulesProvider,
  readParityWorkflowRules,
} from "./workflowParityRules.js";

const RULES = [
  { from: "todo", to: "review", reason: "Finish the work first." },
  { from: null, to: "done", reason: "Nothing starts done." },
];

afterEach(() => {
  window.sessionStorage.clear();
  vi.restoreAllMocks();
});

function seed(value: unknown): void {
  window.sessionStorage.setItem(PARITY_WORKFLOW_RULES_KEY, JSON.stringify(value));
}

describe("readParityWorkflowRules", () => {
  it("reads the injected rules", () => {
    seed(RULES);
    expect(readParityWorkflowRules()).toEqual(RULES);
  });

  it("reads a missing key as no rules", () => {
    expect(readParityWorkflowRules()).toBeNull();
  });

  it("reads malformed keys as no rules", () => {
    for (const broken of [
      "not json",
      JSON.stringify({ from: "todo", to: "review", reason: "x" }),
      JSON.stringify([null]),
      JSON.stringify(["todo"]),
      JSON.stringify([{ from: "todo", to: "review" }]),
      JSON.stringify([{ from: "todo", to: "", reason: "x" }]),
      JSON.stringify([{ from: "todo", to: "review", reason: "" }]),
      JSON.stringify([{ from: 7, to: "review", reason: "x" }]),
      JSON.stringify([{ to: "review", reason: "x" }]),
      JSON.stringify([...RULES, { from: "todo", to: "review" }]),
    ]) {
      window.sessionStorage.setItem(PARITY_WORKFLOW_RULES_KEY, broken);
      expect(readParityWorkflowRules()).toBeNull();
    }
  });

  it("reads no rules in production builds even with the key set", () => {
    seed(RULES);
    expect(readParityWorkflowRules(false)).toBeNull();
  });

  it("reads storage failures as no rules", () => {
    seed(RULES);
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    try {
      expect(readParityWorkflowRules()).toBeNull();
    } finally {
      getItem.mockRestore();
    }
  });
});

describe("parityWorkflowRulesProvider", () => {
  it("refuses exactly the injected hop", () => {
    seed(RULES);
    const rules = parityWorkflowRulesProvider();
    expect(
      rules.refusal({ projectId: "p-1", fromStateId: "todo", toStateId: "review", purpose: "state-picker" })
    ).toEqual({ reason: "Finish the work first." });
    expect(
      rules.refusal({ projectId: "p-1", fromStateId: "todo", toStateId: "done", purpose: "state-picker" })
    ).toBeNull();
    expect(
      rules.refusal({ projectId: "p-1", fromStateId: "review", toStateId: "review", purpose: "board-drop" })
    ).toBeNull();
  });

  it("refuses creation into an injected target", () => {
    seed(RULES);
    const rules = parityWorkflowRulesProvider();
    expect(rules.creationRefusal?.({ projectId: "p-1", columnKey: "done" })).toEqual({
      reason: "Nothing starts done.",
    });
    expect(rules.creationRefusal?.({ projectId: "p-1", columnKey: "review" })).toBeNull();
  });

  it("allows everything without injected rules", () => {
    const rules = parityWorkflowRulesProvider();
    expect(rules.refusal({ projectId: "p-1", fromStateId: "todo", toStateId: "review", purpose: "create" })).toBeNull();
    expect(rules.creationRefusal?.({ projectId: "p-1", columnKey: "done" })).toBeNull();
  });
});
