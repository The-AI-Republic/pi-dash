// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run payloads as the backends produce them. Migration runner/0032
// dropped AgentRun.queue_position, so neither the issue detail's embedded
// run nor a runs-list item carries it; the contracts keep it optional for
// older servers. The guard fails when a contract requires a key either
// backend no longer sends, which is how the issue page and the Runs list
// broke.
//
// Nothing below is a hand copy. Every payload and key list comes from
// the serializers or from output captured off them:
// - Django runs list: the DRF rows captured in fixture FX-RUN-08.
// - Rust runs list: `RUN_WIRE_FIELDS` (runner_runs/shape.rs), which the
//   Rust tests pin to the handler's output.
// - Rust issue run: the bytes `agent_run_to_representation` must emit in
//   serializers_detail.rs `run_replays_bytes_with_diagnostic_and_live_state`.
// - Django issue run: the dict `_serialize_agent_run` returns
//   (apps/api app/serializers/issue.py), with values from the captures above.
// When a serializer changes, this file reads the new version.
import { describe, expect, it } from "vitest";
import type { z } from "zod/mini";

import djangoIssueSerializer from "../../../../apps/api/pi_dash/app/serializers/issue.py?raw";
import runsCapture from "../../../../rust-api/fixtures/runner_runs/fx-run-08-handlers-web.golden.json";
import rustIssueSerializer from "../../../../rust-api/crates/services/src/app_issues/serializers_detail.rs?raw";
import rustRunShape from "../../../../rust-api/crates/services/src/runner_runs/shape.rs?raw";
import { AgentRunSummary } from "./issue-agent.js";
import { AgentRun } from "./runs.js";

type Payload = Record<string, unknown>;
type Schema = { shape: Record<string, z.ZodMiniType> };

/** The string items of `pub const <name>: [&str; N] = [...]`, checked against N. */
function rustConstKeys(source: string, name: string): string[] {
  const match = new RegExp(`pub const ${name}: \\[&str; (\\d+)\\] = \\[([^\\]]*)\\];`).exec(source);
  if (!match?.[1] || match[2] === undefined) throw new Error(`${name} not found`);
  const keys = [...match[2].matchAll(/"([a-z_]+)"/g)].map((item) => item[1] ?? "");
  expect(keys).toHaveLength(Number(match[1]));
  return keys;
}

/** The JSON a Rust test asserts `serde_json::to_string(...)` produces. */
function rustReplayedJson(source: string, testName: string): Payload {
  const start = source.indexOf(`fn ${testName}()`);
  if (start < 0) throw new Error(`${testName} not found`);
  const literal = /"(\{\\"id\\".*?\})",\n/s.exec(source.slice(start));
  if (!literal?.[1]) throw new Error(`${testName} has no replayed JSON`);
  const text = literal[1].replaceAll(/\\\n\s*/g, "").replaceAll(/\\(["\\])/g, "$1");
  return JSON.parse(text) as Payload;
}

/** The keys of the dict literal a Python method returns, in order. */
function pythonReturnedKeys(source: string, method: string): string[] {
  const start = source.indexOf(`def ${method}(`);
  if (start < 0) throw new Error(`${method} not found`);
  const body = /\n(\s+)return \{\n(.*?)\n\1\}/s.exec(source.slice(start));
  if (!body?.[2]) throw new Error(`${method} returns no dict literal`);
  return [...body[2].matchAll(/^\s+"([a-z_]+)":/gm)].map((item) => item[1] ?? "");
}

/** A payload with exactly `keys`, each value taken from the first source that has it. */
function assemble(keys: readonly string[], ...sources: Payload[]): Payload {
  return Object.fromEntries(
    keys.map((key) => {
      const source = sources.find((candidate) => key in candidate);
      if (!source) throw new Error(`no captured value for ${key}`);
      return [key, source[key]];
    })
  );
}

/** Keys the schema rejects when absent. */
function requiredKeys(schema: Schema): string[] {
  return Object.entries(schema.shape)
    .filter(([, field]) => !field.safeParse(undefined).success)
    .map(([key]) => key);
}

/** Wire keys the schema does not model (parsing drops them). */
function unmodelledKeys(schema: Schema, payload: Payload): string[] {
  return Object.keys(payload).filter((key) => !(key in schema.shape));
}

const djangoListRows: Payload[] = [runsCapture.list.golden_row, runsCapture.detail.ok.body];
const [djangoListRow] = djangoListRows as [Payload];

const rustListKeys = rustConstKeys(rustRunShape, "RUN_WIRE_FIELDS");
const rustListRow = assemble(rustListKeys, djangoListRow, { capacity_wait: null });

const rustIssueKeys = rustConstKeys(rustIssueSerializer, "AGENT_RUN_FIELDS");
// The Rust test uses synthetic ids (5555…) that are not RFC 4122 UUIDs;
// swap in the captured Django ids so only the shape is under test.
const replayedIssueRun = rustReplayedJson(rustIssueSerializer, "run_replays_bytes_with_diagnostic_and_live_state");
const rustIssueRun: Payload = {
  ...replayedIssueRun,
  id: djangoListRow.id,
  runner: djangoListRow.pod,
  live_state: { ...(replayedIssueRun.live_state as Payload), observed_run_id: djangoListRow.id },
};

const djangoIssueKeys = pythonReturnedKeys(djangoIssueSerializer, "_serialize_agent_run");
const djangoIssueRun = assemble(djangoIssueKeys, rustIssueRun, djangoListRow);

describe("run payloads as the serializers produce them", () => {
  it("reads every serializer source", () => {
    expect(Object.keys(rustIssueRun)).toEqual(rustIssueKeys);
    expect(djangoIssueKeys).toContain("live_state");
    expect(djangoIssueKeys).not.toContain("queue_position");
    for (const row of djangoListRows) expect(row).not.toHaveProperty("queue_position");
    expect(rustListKeys).not.toContain("queue_position");
    expect(rustIssueKeys).not.toContain("queue_position");
  });

  it("parses the issue detail's embedded run from both backends", () => {
    for (const run of [djangoIssueRun, rustIssueRun]) {
      const parsed = AgentRunSummary.safeParse(run);
      expect(parsed.error?.issues).toBeUndefined();
    }
  });

  it("parses a runs-list item from both backends", () => {
    for (const row of [...djangoListRows, rustListRow]) {
      const parsed = AgentRun.safeParse(row);
      expect(parsed.error?.issues).toBeUndefined();
    }
  });

  it("requires no key either backend does not send", () => {
    const missing = (schema: Schema, payload: Payload) => requiredKeys(schema).filter((key) => !(key in payload));
    expect(missing(AgentRunSummary, djangoIssueRun)).toEqual([]);
    expect(missing(AgentRunSummary, rustIssueRun)).toEqual([]);
    for (const row of [...djangoListRows, rustListRow]) expect(missing(AgentRun, row)).toEqual([]);
  });

  it("models every key the backends send except the known ones it ignores", () => {
    // failure_reason drives the server's error_diagnostic; the client
    // reads the diagnostic. A new key in this list must be modelled or
    // listed here on purpose.
    const ignored = ["failure_reason"];
    expect(unmodelledKeys(AgentRunSummary, djangoIssueRun)).toEqual(ignored);
    expect(unmodelledKeys(AgentRunSummary, rustIssueRun)).toEqual(ignored);
    for (const row of [...djangoListRows, rustListRow]) expect(unmodelledKeys(AgentRun, row)).toEqual(ignored);
  });

  it("does not require queue_position but still accepts it from older servers", () => {
    expect(requiredKeys(AgentRunSummary)).not.toContain("queue_position");
    expect(requiredKeys(AgentRun)).not.toContain("queue_position");
    expect(AgentRunSummary.safeParse({ ...djangoIssueRun, queue_position: 3 }).success).toBe(true);
    expect(AgentRun.safeParse({ ...djangoListRow, queue_position: null }).success).toBe(true);
  });
});
