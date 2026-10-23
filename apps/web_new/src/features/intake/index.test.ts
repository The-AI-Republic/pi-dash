// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature's public surface: every name a route or a sibling slice
// imports from here resolves (NEWFRONT-689). Importing the barrel is the
// whole assertion; a missing export fails the module load.
import { describe, expect, it } from "vitest";

import * as intake from "./index.js";

describe("intake public API", () => {
  it("exports the shell, gate, tabs, data layer and pure model", () => {
    for (const name of [
      "IntakeScreen",
      "IntakeGate",
      "IntakeEmptyState",
      "IntakeCreateDialog",
      "IntakeTabs",
      "IntakeDetail",
      "IntakeDetailHeader",
      "IntakeProperties",
      "useIntakeList",
      "useCreateIntakeRequest",
      "prefetchIntakeList",
      "intakeListQueryOptions",
      "intakeRows",
      "intakeTotal",
      "listStatuses",
      "useIntakeDetail",
      "intakeDetailQueryOptions",
      "intakeDetailWorkItem",
      "isMissingIntakeDetail",
      "useUpdateIntakeIssue",
      "paintIntakeDetailPatch",
      "neighborIntakeId",
      "suppressIntakeArrowKey",
      "INTAKE_NAV_OVERLAY_SELECTOR",
      "selectIntakeEditability",
      "isResolvedIntakeStatus",
      "intakeKeys",
      "intakeHref",
      "retiredInboxHref",
      "intakeFeatureSettingsHref",
      "parseIntakeSearch",
      "intakeSearchFor",
      "intakeSearchAfterRemove",
      "settleIntakeSearch",
      "parseIntakeTab",
      "statusParamForTab",
      "shouldShowOpenBadge",
      "intakeDocumentTitle",
      "selectIntakeEmptyState",
      "emptyIntakeCreateForm",
      "resetIntakeCreateForm",
      "buildIntakeCreatePayload",
      "intakeTitleError",
      "isIntakeTitleSubmittable",
      "INTAKE_CREATE_DIALOG_TITLE",
      "INTAKE_CREATE_SUBMIT_LABEL",
      "INTAKE_CREATE_MORE_LABEL",
    ]) {
      expect(intake, name).toHaveProperty(name);
    }
    expect(intake.DEFAULT_INTAKE_SEARCH).toEqual({ currentTab: "open" });
    expect(intake.INTAKE_AREA_LABEL).toBe("Intake");
    expect(intake.INTAKE_LIST_PANEL_ID).toBe("intake-list-panel");
  });
});
