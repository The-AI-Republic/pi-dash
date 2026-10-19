// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod form rules (NEWFRONT-542, RUN-013–015). The name helpers strip and
// rebuild the project prefix, the edit patch carries only what changed, and
// delete failures prefer the guard sentence for their code.
import { ApiError, type Pod } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  POD_CREATE_FAILED,
  POD_DELETE_FAILED,
  POD_DELETE_GUARDS,
  POD_NAME_REQUIRED,
  POD_PROJECT_REQUIRED,
  POD_UPDATE_FAILED,
  podDeleteErrorMessage,
  podNameSuffix,
  podNameWithPrefix,
  podUpdatePatch,
  podWriteErrorMessage,
  seedPodEdit,
  validatePodDraft,
  validatePodName,
} from "./pods.js";

function pod(overrides: Partial<Pod> = {}): Pod {
  return {
    id: "pod-1",
    name: "WEB_beefy",
    description: "runs on the beefy boxes",
    is_default: false,
    workspace: "ws-1",
    project: "prj-1",
    project_identifier: "WEB",
    created_by: null,
    runner_count: 2,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("pod name helpers", () => {
  it("strips only the project's own prefix", () => {
    expect(podNameSuffix("WEB_beefy", "WEB")).toBe("beefy");
    expect(podNameSuffix("WEB_team_beefy", "WEB")).toBe("team_beefy");
  });

  it("leaves a name without the prefix whole", () => {
    expect(podNameSuffix("beefy", "WEB")).toBe("beefy");
    expect(podNameSuffix("OTHER_beefy", "WEB")).toBe("OTHER_beefy");
    expect(podNameSuffix("", "WEB")).toBe("");
  });

  it("rebuilds the stored name from a suffix", () => {
    expect(podNameWithPrefix("beefy", "WEB")).toBe("WEB_beefy");
    expect(podNameSuffix(podNameWithPrefix("x-y", "ARC"), "ARC")).toBe("x-y");
  });

  it("trims a name and rejects a blank one", () => {
    expect(validatePodName("  beefy  ")).toEqual({ value: "beefy" });
    expect(validatePodName("   ")).toEqual({ error: POD_NAME_REQUIRED });
    expect(validatePodName("")).toEqual({ error: POD_NAME_REQUIRED });
  });
});

describe("validatePodDraft", () => {
  it("names both missing fields on an empty submit", () => {
    expect(validatePodDraft({ projectId: "", name: "" })).toEqual({
      projectId: POD_PROJECT_REQUIRED,
      name: POD_NAME_REQUIRED,
    });
    expect(validatePodDraft({ projectId: "   ", name: "  " })).toEqual({
      projectId: POD_PROJECT_REQUIRED,
      name: POD_NAME_REQUIRED,
    });
  });

  it("accepts a project and a trimmed name", () => {
    expect(validatePodDraft({ projectId: "prj-1", name: " beefy " })).toEqual({});
  });

  it("reports one missing field alone", () => {
    expect(validatePodDraft({ projectId: "", name: "beefy" })).toEqual({ projectId: POD_PROJECT_REQUIRED });
    expect(validatePodDraft({ projectId: "prj-1", name: " " })).toEqual({ name: POD_NAME_REQUIRED });
  });
});

describe("seedPodEdit", () => {
  it("shows the suffix, the stored description and the default flag", () => {
    expect(seedPodEdit(pod({ is_default: true }))).toEqual({
      name: "beefy",
      description: "runs on the beefy boxes",
      isDefault: true,
    });
  });
});

describe("podUpdatePatch", () => {
  const seed = seedPodEdit(pod());

  it("returns null for an untouched form", () => {
    expect(podUpdatePatch(seed, { ...seed })).toBeNull();
    expect(podUpdatePatch(seed, { ...seed, name: "  beefy  " })).toBeNull();
    expect(podUpdatePatch(seed, { ...seed, description: " runs on the beefy boxes " })).toBeNull();
  });

  it("sends just the name for a rename", () => {
    expect(podUpdatePatch(seed, { ...seed, name: " heavier " })).toEqual({ name: "heavier" });
  });

  it("sends just the description, including a cleared one", () => {
    expect(podUpdatePatch(seed, { ...seed, description: "new home" })).toEqual({ description: "new home" });
    expect(podUpdatePatch(seed, { ...seed, description: "   " })).toEqual({ description: "" });
  });

  it("sends just the default flag when promoted", () => {
    expect(podUpdatePatch(seed, { ...seed, isDefault: true })).toEqual({ is_default: true });
  });

  it("combines every changed field", () => {
    expect(podUpdatePatch(seed, { name: "x", description: "y", isDefault: true })).toEqual({
      name: "x",
      description: "y",
      is_default: true,
    });
  });
});

describe("podDeleteErrorMessage", () => {
  it("uses each guard's own sentence", () => {
    for (const code of Object.keys(POD_DELETE_GUARDS)) {
      const error = new ApiError({ status: 409, code, message: "raw server text" });
      expect(podDeleteErrorMessage(error)).toBe(POD_DELETE_GUARDS[code]);
    }
    expect(podDeleteErrorMessage(new ApiError({ status: 409, code: "pod_has_runners", message: "" }))).toContain(
      "pod's runners"
    );
    expect(
      podDeleteErrorMessage(new ApiError({ status: 409, code: "default_pod_undeletable", message: "" }))
    ).toContain("default pod");
    expect(podDeleteErrorMessage(new ApiError({ status: 409, code: "pod_has_active_runs", message: "" }))).toContain(
      "runs in flight"
    );
  });

  it("falls back to the server message, then the generic sentence", () => {
    expect(podDeleteErrorMessage(new ApiError({ status: 500, code: "http", message: "server said no" }))).toBe(
      "server said no"
    );
    expect(podDeleteErrorMessage(new ApiError({ status: 500, code: "http", message: "  " }))).toBe(POD_DELETE_FAILED);
    expect(podDeleteErrorMessage(new Error("nope"))).toBe(POD_DELETE_FAILED);
    expect(podDeleteErrorMessage("nope")).toBe(POD_DELETE_FAILED);
  });
});

describe("podWriteErrorMessage", () => {
  it("prefers the server's sentence", () => {
    expect(
      podWriteErrorMessage(new ApiError({ status: 400, code: "http", message: "name taken" }), POD_CREATE_FAILED)
    ).toBe("name taken");
  });

  it("uses the caller's fallback for other failures", () => {
    expect(podWriteErrorMessage(new Error("boom"), POD_CREATE_FAILED)).toBe(POD_CREATE_FAILED);
    expect(podWriteErrorMessage(new ApiError({ status: 0, code: "http", message: "" }), POD_UPDATE_FAILED)).toBe(
      POD_UPDATE_FAILED
    );
  });
});
