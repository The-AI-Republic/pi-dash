// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Form rules (RUN-006/007/009): validation text, the project/pod cascade,
// machine auto-selection and snap-back, agent gating and the failure copy.
import type { Pod } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  agentGating,
  autoSelectMachine,
  connectedMachines,
  DAEMON_TOO_OLD_TEXT,
  DEFAULT_POD,
  DEFAULT_POD_LABEL,
  defaultModelFor,
  emptyValues,
  isUnknownAgentError,
  machineLabel,
  MANUAL_MACHINE,
  NAME_RULE_TEXT,
  podOptionLabel,
  podsForProject,
  projectForPod,
  PROJECT_REQUIRED_TEXT,
  remoteFailureText,
  remoteStatusError,
  settleAgent,
  settleMachineSelection,
  settlePodSelection,
  validateValues,
} from "./model.js";
import { PRESELECTED_MODEL } from "./catalog.js";

const pod = (name: string, project: string): Pod =>
  ({
    id: `id-${name}`,
    name,
    description: "",
    is_default: false,
    workspace: "ws",
    project: `pid-${project}`,
    project_identifier: project,
    created_by: null,
    runner_count: 0,
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
  }) as Pod;

const PODS = [pod("acme-alpha", "ACME"), pod("beta-alpha", "BETA")];
const PROJECTS = [
  { id: "p1", name: "Acme", identifier: "ACME" },
  { id: "p2", name: "Beta", identifier: "BETA" },
];

describe("initial values", () => {
  it("starts on the manual path with Claude Code's pinned model", () => {
    expect(emptyValues()).toEqual({
      devMachineId: MANUAL_MACHINE,
      projectIdentifier: "",
      podName: DEFAULT_POD,
      name: "",
      workingDir: "",
      agent: "claude-code",
      model: PRESELECTED_MODEL["claude-code"],
    });
    expect(DEFAULT_POD).toBe("");
    expect(DEFAULT_POD_LABEL).toBe("(default pod)");
  });
});

describe("validateValues", () => {
  it("requires a project", () => {
    expect(validateValues({ projectIdentifier: "", name: "" })).toEqual({
      projectIdentifier: PROJECT_REQUIRED_TEXT,
    });
    expect(validateValues({ projectIdentifier: "   ", name: "" }).projectIdentifier).toBe(PROJECT_REQUIRED_TEXT);
  });

  it("accepts a blank name (auto-assigned) and a valid one", () => {
    expect(validateValues({ projectIdentifier: "ACME", name: "" })).toEqual({});
    expect(validateValues({ projectIdentifier: "ACME", name: "good_name-1.2" })).toEqual({});
    expect(validateValues({ projectIdentifier: "ACME", name: "_x" })).toEqual({});
  });

  it("rejects spaces, a bad start and over-long names with one message", () => {
    for (const name of ["has space", "-badstart", ".dot", "x".repeat(129)]) {
      expect(validateValues({ projectIdentifier: "ACME", name }).name).toBe(NAME_RULE_TEXT);
    }
  });

  it("trims before validating, so outer padding is harmless but inner spaces are not", () => {
    expect(validateValues({ projectIdentifier: "ACME", name: "  spaced  " })).toEqual({});
    expect(validateValues({ projectIdentifier: "ACME", name: "  has space  " }).name).toBe(NAME_RULE_TEXT);
    expect(validateValues({ projectIdentifier: "ACME", name: "  ok_name  " })).toEqual({});
  });
});

describe("machine picker", () => {
  const machines = [
    { id: "m1", label: "", host_label: "host-one", id2: "x", control_online: true, revoked_at: null },
    { id: "m2", label: "offline", host_label: "host-two", control_online: false, revoked_at: null },
    { id: "m3", label: "revoked", host_label: "host-three", control_online: true, revoked_at: "2024-01-01" },
  ];

  it("filters to online, non-revoked machines", () => {
    expect(connectedMachines(machines).map((row) => row.id)).toEqual(["m1"]);
    expect(connectedMachines(undefined)).toEqual([]);
  });

  it("labels a machine by label, host label or id", () => {
    expect(machineLabel({ id: "m", label: "named", host_label: "host" })).toBe("named");
    expect(machineLabel({ id: "m", label: "", host_label: "host" })).toBe("host");
    expect(machineLabel({ id: "m", label: "", host_label: "" })).toBe("m");
  });

  it("auto-selects the first machine once when untouched", () => {
    expect(autoSelectMachine(MANUAL_MACHINE, [{ id: "m1", label: "one" }], false)).toBe("m1");
    expect(autoSelectMachine(MANUAL_MACHINE, [{ id: "m1", label: "one" }], true)).toBeNull();
    expect(autoSelectMachine(MANUAL_MACHINE, [], false)).toBeNull();
    expect(autoSelectMachine("m2", [{ id: "m1", label: "one" }], false)).toBeNull();
  });

  it("snaps a dropped machine back to manual but keeps a live one", () => {
    const live = [{ id: "m1", label: "one" }];
    expect(settleMachineSelection("m1", live)).toBe("m1");
    expect(settleMachineSelection("gone", live)).toBe(MANUAL_MACHINE);
    expect(settleMachineSelection(MANUAL_MACHINE, live)).toBe(MANUAL_MACHINE);
  });
});

describe("project/pod cascade", () => {
  it("shows every pod before a project is picked", () => {
    expect(podsForProject(PODS, "")).toEqual(PODS);
    expect(podsForProject(undefined, "")).toEqual([]);
  });

  it("narrows to the chosen project's pods", () => {
    expect(podsForProject(PODS, "ACME").map((row) => row.name)).toEqual(["acme-alpha"]);
  });

  it("backfills the project from a pod-first pick", () => {
    expect(projectForPod("beta-alpha", PODS, PROJECTS)).toBe("BETA");
    expect(projectForPod("beta-alpha", PODS, [])).toBeNull();
    expect(projectForPod("missing", PODS, PROJECTS)).toBeNull();
    expect(projectForPod(DEFAULT_POD, PODS, PROJECTS)).toBeNull();
  });

  it("clears an ill-fitting pod on project change but keeps the default", () => {
    expect(settlePodSelection("beta-alpha", "ACME", PODS)).toBe(DEFAULT_POD);
    expect(settlePodSelection("acme-alpha", "ACME", PODS)).toBe("acme-alpha");
    expect(settlePodSelection(DEFAULT_POD, "ACME", PODS)).toBe(DEFAULT_POD);
  });

  it("suffixes project pods and leaves workspace pods bare", () => {
    expect(podOptionLabel(pod("acme-alpha", "ACME"))).toBe("acme-alpha (ACME)");
    expect(podOptionLabel(pod("shared", ""))).toBe("shared");
  });
});

describe("agent gating", () => {
  it("gates nothing without a machine or without an advertised set", () => {
    expect(agentGating(undefined)).toEqual({ disabled: [], hint: null });
    expect(agentGating({ id: "m", label: "m", supportedAgents: [] })).toEqual({ disabled: [], hint: null });
    expect(agentGating({ id: "m", label: "m", supportedAgents: undefined })).toEqual({ disabled: [], hint: null });
  });

  it("disables unsupported agents and explains the upgrade", () => {
    const gating = agentGating({ id: "m", label: "m", supportedAgents: ["claude-code", "codex"] });
    expect(gating.disabled).toEqual(["cursor-agent", "open-claw", "grok", "muse-code"]);
    expect(gating.hint).toContain("Update pidash");
  });

  it("hides the hint when the machine supports everything", () => {
    const gating = agentGating({
      id: "m",
      label: "m",
      supportedAgents: ["claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"],
    });
    expect(gating).toEqual({ disabled: [], hint: null });
  });

  it("snaps to the first supported agent when the current one is unsupported", () => {
    expect(settleAgent("muse-code", { id: "m", label: "m", supportedAgents: ["codex", "grok"] })).toBe("codex");
    expect(settleAgent("grok", { id: "m", label: "m", supportedAgents: ["codex", "grok"] })).toBe("grok");
    expect(settleAgent("muse-code", undefined)).toBe("muse-code");
    expect(settleAgent("muse-code", { id: "m", label: "m", supportedAgents: [] })).toBe("muse-code");
    expect(settleAgent("muse-code", { id: "m", label: "m", supportedAgents: ["alien"] })).toBe("muse-code");
  });

  it("resets the model to the agent's preselected default", () => {
    expect(defaultModelFor("claude-code")).toBe("claude-opus-4-8");
    expect(defaultModelFor("codex")).toBe("default");
  });
});

describe("remote failures", () => {
  it("distinguishes offline, unsupported agents and generic failures", () => {
    expect(remoteFailureText("machine_offline")).toContain("went offline");
    expect(remoteFailureText("unsupported_agent")).toBe(DAEMON_TOO_OLD_TEXT);
    expect(remoteFailureText('unknown agent kind "muse-code"')).toBe(DAEMON_TOO_OLD_TEXT);
    expect(remoteFailureText("boom")).toBe("boom");
    expect(remoteFailureText(undefined)).toContain("Could not reach the cloud");
  });

  it("reads the server reason out of the shared client's http code", () => {
    expect(remoteFailureText("http", "machine_offline")).toContain("went offline");
    expect(remoteFailureText("http", "unsupported_agent")).toBe(DAEMON_TOO_OLD_TEXT);
    expect(remoteFailureText("http", "boom")).toBe("boom");
    expect(remoteFailureText("http", "")).toContain("Could not reach the cloud");
    expect(remoteFailureText("network", "network down")).toContain("Could not reach the cloud");
    expect(remoteFailureText("unsupported_agent", "ignored")).toBe(DAEMON_TOO_OLD_TEXT);
  });

  it("maps the daemon's raw write-back to the upgrade hint", () => {
    expect(isUnknownAgentError('unknown agent kind "grok"')).toBe(true);
    expect(isUnknownAgentError("other")).toBe(false);
    expect(isUnknownAgentError(undefined)).toBe(false);
    expect(remoteStatusError('unknown agent kind "grok"')).toBe(DAEMON_TOO_OLD_TEXT);
    expect(remoteStatusError("parity boom")).toBe("parity boom");
    expect(remoteStatusError("machine_offline")).toContain("went offline");
    expect(remoteStatusError(undefined)).toBe("");
  });
});
