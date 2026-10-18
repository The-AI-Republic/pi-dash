// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  AGENT_OPTION_PREFIX,
  AGENT_ROW_LABELS,
  buildExecutionTargets,
  CLOUD_EXECUTOR,
  EstimatePicker,
  executionSelectionId,
  executionTargetLabel,
  executionTargetPatch,
  formatAvailabilityReason,
  IntakeStatePicker,
  isAgentRowKind,
  LOCAL_EXECUTOR,
  MANAGED_EXECUTOR,
  NO_ESTIMATE_ID,
  parseExecutionSelectionId,
  pickerEstimatesQueryOptions,
  pickerIntakeStatesQueryOptions,
  pickerPodsQueryOptions,
  pickerProjectQueryOptions,
  PodPicker,
  POD_OPTION_PREFIX,
  selectDefaultPod,
  selectEstimateOptions,
  selectIntakeFallback,
  StateGlyph,
  usePickerEstimates,
  usePickerIntakeStates,
  usePickerPods,
  usePickerProject,
} from "./index.js";

describe("pickers barrel", () => {
  it("re-exports the estimate, intake-state and execution-target pickers", () => {
    expect(EstimatePicker).toBeDefined();
    expect(IntakeStatePicker).toBeDefined();
    expect(PodPicker).toBeDefined();
    expect(StateGlyph).toBeDefined();
    expect(NO_ESTIMATE_ID).toBe("none");
    expect(selectEstimateOptions).toBeDefined();
    expect(selectIntakeFallback).toBeDefined();
    expect(AGENT_OPTION_PREFIX).toBe("agent:");
    expect(POD_OPTION_PREFIX).toBe("pod:");
    expect(AGENT_ROW_LABELS).toBeDefined();
    expect(CLOUD_EXECUTOR).toBe("cloud_agent");
    expect(MANAGED_EXECUTOR).toBe("managed_runner");
    expect(LOCAL_EXECUTOR).toBe("local_runner");
    expect(buildExecutionTargets).toBeDefined();
    expect(executionSelectionId).toBeDefined();
    expect(executionTargetLabel).toBeDefined();
    expect(executionTargetPatch).toBeDefined();
    expect(formatAvailabilityReason).toBeDefined();
    expect(isAgentRowKind).toBeDefined();
    expect(parseExecutionSelectionId).toBeDefined();
    expect(selectDefaultPod).toBeDefined();
    expect(pickerEstimatesQueryOptions).toBeDefined();
    expect(pickerIntakeStatesQueryOptions).toBeDefined();
    expect(pickerPodsQueryOptions).toBeDefined();
    expect(pickerProjectQueryOptions).toBeDefined();
    expect(usePickerEstimates).toBeDefined();
    expect(usePickerIntakeStates).toBeDefined();
    expect(usePickerPods).toBeDefined();
    expect(usePickerProject).toBeDefined();
  });
});
