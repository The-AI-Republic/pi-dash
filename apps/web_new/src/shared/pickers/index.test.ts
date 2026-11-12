// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  buildExecutionTargets,
  EstimatePicker,
  executionSelectionId,
  EXECUTION_TARGET_LABEL,
  executionTargetPatch,
  IntakeStatePicker,
  LOCAL_EXECUTOR,
  NO_ESTIMATE_ID,
  parseExecutionSelectionId,
  pickerEstimatesQueryOptions,
  pickerIntakeStatesQueryOptions,
  pickerPodsQueryOptions,
  pickerProjectQueryOptions,
  pickerRunnersQueryOptions,
  PodPicker,
  POD_OPTION_PREFIX,
  podOptionLabel,
  RUNNER_OPTION_PREFIX,
  runnerReasonMessage,
  runnerStatusLine,
  runnerWaitNote,
  selectDefaultPod,
  selectEstimateOptions,
  selectIntakeFallback,
  selectRunnerOwnerName,
  StateGlyph,
  usePickerEstimates,
  usePickerIntakeStates,
  usePickerPods,
  usePickerProject,
  usePickerRunners,
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
    expect(POD_OPTION_PREFIX).toBe("pod:");
    expect(RUNNER_OPTION_PREFIX).toBe("runner:");
    expect(podOptionLabel).toBeDefined();
    expect(runnerReasonMessage).toBeDefined();
    expect(runnerStatusLine).toBeDefined();
    expect(runnerWaitNote).toBeDefined();
    expect(selectRunnerOwnerName).toBeDefined();
    expect(LOCAL_EXECUTOR).toBe("local_runner");
    expect(buildExecutionTargets).toBeDefined();
    expect(executionSelectionId).toBeDefined();
    expect(EXECUTION_TARGET_LABEL).toBe("Runs on");
    expect(executionTargetPatch).toBeDefined();
    expect(parseExecutionSelectionId).toBeDefined();
    expect(selectDefaultPod).toBeDefined();
    expect(pickerEstimatesQueryOptions).toBeDefined();
    expect(pickerIntakeStatesQueryOptions).toBeDefined();
    expect(pickerPodsQueryOptions).toBeDefined();
    expect(pickerProjectQueryOptions).toBeDefined();
    expect(pickerRunnersQueryOptions).toBeDefined();
    expect(usePickerEstimates).toBeDefined();
    expect(usePickerIntakeStates).toBeDefined();
    expect(usePickerPods).toBeDefined();
    expect(usePickerProject).toBeDefined();
    expect(usePickerRunners).toBeDefined();
  });
});
