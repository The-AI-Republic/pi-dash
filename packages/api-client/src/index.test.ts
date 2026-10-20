// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  checkProjectIdentifier,
  ProjectIdentifierCheck,
  ProjectIdentifierMatch,
  updateProject,
  updateProjectSettings,
} from "./index.js";
import type { ProjectUpdateInput } from "./index.js";
import {
  checkProjectIdentifier as directCheck,
  ProjectIdentifierCheck as DirectCheck,
  ProjectIdentifierMatch as DirectMatch,
  updateProject as directUpdate,
} from "./contracts/project-management.js";
import { updateProjectSettings as directSettingsUpdate } from "./contracts/project-settings-general.js";
import {
  billingPlanAction,
  createPersonalApiToken,
  createWorkspaceWebhook,
  getGithubIntegration,
  listPersonalApiTokens,
  listWorkspaceWebhooks,
  PersonalApiToken as EntryPersonalApiToken,
  WorkspaceWebhook as EntryWorkspaceWebhook,
} from "./index.js";
import { billingPlanAction as moduleBillingPlanAction } from "./contracts/billing-plans.js";
import {
  createPersonalApiToken as moduleCreatePersonalApiToken,
  listPersonalApiTokens as moduleListPersonalApiTokens,
  PersonalApiToken as ModulePersonalApiToken,
} from "./contracts/api-tokens.js";
import { getGithubIntegration as moduleGetGithubIntegration } from "./contracts/integrations.js";
import {
  createWorkspaceWebhook as moduleCreateWorkspaceWebhook,
  listWorkspaceWebhooks as moduleListWorkspaceWebhooks,
  WorkspaceWebhook as ModuleWorkspaceWebhook,
} from "./contracts/webhooks.js";

describe("@pidash/api-client entry", () => {
  it("imports cleanly", async () => {
    const entry = await import("./index");
    expect(entry).toBeDefined();
  });

  it("re-exports the project update and identifier contracts", () => {
    expect(checkProjectIdentifier).toBe(directCheck);
    expect(updateProject).toBe(directUpdate);
    expect(updateProjectSettings).toBe(directSettingsUpdate);
    expect(ProjectIdentifierCheck).toBe(DirectCheck);
    expect(ProjectIdentifierMatch).toBe(DirectMatch);
    const input: ProjectUpdateInput = { description: "typed" };
    expect(input.description).toBe("typed");
  });

  it("re-exports the workspace settings developer, webhook, token and billing contracts", () => {
    expect(getGithubIntegration).toBe(moduleGetGithubIntegration);
    expect(listWorkspaceWebhooks).toBe(moduleListWorkspaceWebhooks);
    expect(createWorkspaceWebhook).toBe(moduleCreateWorkspaceWebhook);
    expect(EntryWorkspaceWebhook).toBe(ModuleWorkspaceWebhook);
    expect(listPersonalApiTokens).toBe(moduleListPersonalApiTokens);
    expect(createPersonalApiToken).toBe(moduleCreatePersonalApiToken);
    expect(EntryPersonalApiToken).toBe(ModulePersonalApiToken);
    expect(billingPlanAction).toBe(moduleBillingPlanAction);
  });
});
