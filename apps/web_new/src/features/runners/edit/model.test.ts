// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner edit rules: the seed from the row, the changed subset, gating and
// the refusal text.
import type { Runner } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { DEFAULT_MODEL_ID, MODEL_CHOICES } from "../addRunner/catalog.js";
import {
  EDIT_RUNNER_BUSY,
  EDIT_RUNNER_OFFLINE,
  canEditRunner,
  choicesWithCurrent,
  machineFieldsBlocker,
  modelIdFor,
  reconfigureErrorText,
  runnerAgentOf,
  runnerEditPlan,
  runnerReportsModel,
  seedRunnerEdit,
  validateRunnerName,
  type RunnerEditValues,
} from "./model.js";

const runner = (overrides: Partial<Runner> = {}): Runner =>
  ({
    id: "r-1",
    name: "alpha",
    status: "online",
    owner: "u-1",
    provisioning: "manual",
    revoked_at: null,
    dev_machine: "m-1",
    capabilities: ["agent:codex"],
    dev_metadata: { working_dir: "/w", model: "gpt-5.5", reasoning_effort: "high" },
    ...overrides,
  }) as Runner;

const codex = MODEL_CHOICES.codex;

describe("seed", () => {
  it("reads the agent from the snake_case capability", () => {
    expect(runnerAgentOf(runner({ capabilities: ["agent:claude_code"] }))).toBe("claude-code");
    expect(runnerAgentOf(runner({ capabilities: ["agent:open_claw"] }))).toBe("open-claw");
    expect(runnerAgentOf(runner({ capabilities: ["agent:engine"] }))).toBeNull();
    expect(runnerAgentOf(runner({ capabilities: [] }))).toBeNull();
  });

  it("maps the reported model and effort onto a picker id", () => {
    expect(seedRunnerEdit(runner(), codex)).toEqual({
      name: "alpha",
      workingDir: "/w",
      agent: "codex",
      modelId: "gpt-5.5:high",
    });
    expect(modelIdFor(codex, undefined, undefined)).toBe(DEFAULT_MODEL_ID);
  });

  it("keeps a model the catalog no longer lists as a current choice", () => {
    const choices = choicesWithCurrent(codex, "gpt-4-legacy", "low");
    const id = modelIdFor(choices, "gpt-4-legacy", "low");
    expect(choices.find((choice) => choice.id === id)?.label).toBe("gpt-4-legacy (low, current)");
    expect(choicesWithCurrent(codex, "gpt-5.5", "high")).toHaveLength(codex.length);
  });

  it("tells whether the daemon reports its model", () => {
    expect(runnerReportsModel(runner())).toBe(true);
    expect(runnerReportsModel(runner({ dev_metadata: { working_dir: "/w" } }))).toBe(false);
  });
});

describe("plan", () => {
  const seed: RunnerEditValues = { name: "alpha", workingDir: "/w", agent: "codex", modelId: "gpt-5.5:high" };

  it("sends nothing for an untouched form", () => {
    expect(runnerEditPlan(seed, seed, codex)).toEqual({ name: null, reconfigure: null });
  });

  it("renames on the server only", () => {
    expect(runnerEditPlan(seed, { ...seed, name: " beta " }, codex)).toEqual({ name: "beta", reconfigure: null });
  });

  it("sends a new model with its effort, and clears the effort with the default", () => {
    expect(runnerEditPlan(seed, { ...seed, modelId: "gpt-5.4:low" }, codex).reconfigure).toEqual({
      model: "gpt-5.4",
      reasoning_effort: "low",
    });
    expect(runnerEditPlan(seed, { ...seed, modelId: DEFAULT_MODEL_ID }, codex).reconfigure).toEqual({
      model: "",
      reasoning_effort: "",
    });
  });

  it("carries the model choice with a new agent", () => {
    const plan = runnerEditPlan(
      seed,
      { ...seed, agent: "claude-code", modelId: "claude-opus-5-5" },
      MODEL_CHOICES["claude-code"]
    );
    expect(plan.reconfigure).toEqual({ agent: "claude-code", model: "claude-opus-5-5", reasoning_effort: "" });
  });

  it("ignores a blank working directory and sends a changed one", () => {
    expect(runnerEditPlan(seed, { ...seed, workingDir: "  " }, codex).reconfigure).toBeNull();
    expect(runnerEditPlan(seed, { ...seed, workingDir: " /x " }, codex).reconfigure).toEqual({ working_dir: "/x" });
  });
});

describe("gating", () => {
  it("offers Edit to the owner of a live, non-engine runner", () => {
    expect(canEditRunner(runner(), "u-1")).toBe(true);
    expect(canEditRunner(runner(), "u-2")).toBe(false);
    expect(canEditRunner(runner(), null)).toBe(false);
    expect(canEditRunner(runner({ provisioning: "desktop_bundled" }), "u-1")).toBe(false);
    expect(canEditRunner(runner({ revoked_at: "2026-11-18T00:00:00Z" }), "u-1")).toBe(false);
  });

  it("locks the machine fields while offline, busy or machine-less", () => {
    expect(machineFieldsBlocker(runner())).toBeNull();
    expect(machineFieldsBlocker(runner({ status: "offline" }))).toBe(EDIT_RUNNER_OFFLINE);
    expect(machineFieldsBlocker(runner({ status: "busy" }))).toBe(EDIT_RUNNER_BUSY);
    expect(machineFieldsBlocker(runner({ dev_machine: null }))).toContain("without a dev machine");
  });

  it("validates names like the server", () => {
    expect(validateRunnerName(" ok_1.a-b ")).toEqual({ value: "ok_1.a-b" });
    expect(validateRunnerName("")).toHaveProperty("error");
    expect(validateRunnerName("-dash")).toHaveProperty("error");
    expect(validateRunnerName("has space")).toHaveProperty("error");
  });

  it("explains refusals and passes the daemon's own reason through", () => {
    expect(reconfigureErrorText("machine_offline")).toBe(EDIT_RUNNER_OFFLINE);
    expect(reconfigureErrorText("runner_busy")).toBe(EDIT_RUNNER_BUSY);
    expect(reconfigureErrorText("the Codex CLI (codex) was not found on this machine; install it first")).toContain(
      "not found"
    );
    expect(reconfigureErrorText(undefined)).toContain("could not apply");
  });
});
