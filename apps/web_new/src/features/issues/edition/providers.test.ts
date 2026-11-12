// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud provider seams for the edition-only issues surfaces: registration
// replaces the whole record, null clears it, and the community build leaves
// every slot absent.
import { afterEach, describe, expect, it } from "vitest";

import {
  getEditionIssuesProviders,
  registerEditionIssuesProviders,
  resetEditionIssuesProviders,
  type EditionIssuesProviders,
} from "./providers.js";

afterEach(() => {
  resetEditionIssuesProviders();
});

describe("edition issues providers", () => {
  it("starts empty, so every edition surface renders nothing", () => {
    expect(getEditionIssuesProviders()).toEqual({});
    expect(getEditionIssuesProviders().workflowRules).toBeUndefined();
    expect(getEditionIssuesProviders().detailBadges).toBeUndefined();
    expect(getEditionIssuesProviders().issueTypeDetail).toBeUndefined();
  });

  it("registers a record and reads it back", () => {
    const providers: EditionIssuesProviders = {
      workflowRules: { refusal: () => null },
    };
    registerEditionIssuesProviders(providers);
    expect(getEditionIssuesProviders()).toBe(providers);
  });

  it("replaces a previous registration rather than merging into it", () => {
    registerEditionIssuesProviders({ workflowRules: { refusal: () => null } });
    registerEditionIssuesProviders({ teamProjects: async () => [] });
    const current = getEditionIssuesProviders();
    expect(current.workflowRules).toBeUndefined();
    expect(current.teamProjects).toBeDefined();
  });

  it("clears with null", () => {
    registerEditionIssuesProviders({ workflowRules: { refusal: () => null } });
    registerEditionIssuesProviders(null);
    expect(getEditionIssuesProviders()).toEqual({});
  });

  it("forgets a registration on reset", () => {
    registerEditionIssuesProviders({ workItemTypes: async () => [] });
    resetEditionIssuesProviders();
    expect(getEditionIssuesProviders()).toEqual({});
  });
});
