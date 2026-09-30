// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle driver (NEWFRONT-19): implements the parity driver interface
// against apps/web. Selectors follow the sign-in card behavior observed on
// the running old app: the entry route renders the email step, a submit
// moves to the password step, and the password submit posts the native
// form, landing in the workspace. Later oracle issues extend this driver
// (never fork it) as new areas need new actions.
import type { Locator, Page } from "@playwright/test";
import type { ParityDriver, ParityTarget } from "./parity-driver";

/** Stable hooks the oracle driver relies on in apps/web. */
export const WEB_TEST_IDS = {
  issueName: "parity-issue-name",
} as const;

export class WebDriver implements ParityDriver {
  readonly target: ParityTarget = "web";
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  async openEntry(): Promise<void> {
    await this.page.goto("/");
    await this.page.getByPlaceholder("name@company.com").first().waitFor();
  }

  private submitOf(form: Locator): Locator {
    return form.locator('button[type="submit"]');
  }

  async signInWithPassword(email: string, password: string): Promise<void> {
    const page = this.page;
    const emailField = page.getByPlaceholder("name@company.com").first();
    await emailField.fill(email);
    const emailForm = page.locator("form", { has: emailField });
    await this.submitOf(emailForm).click();
    const passwordField = page.getByPlaceholder("Enter password");
    await passwordField.waitFor();
    await passwordField.fill(password);
    const passwordForm = page.locator("form", { has: passwordField });
    // The old app posts the native form, so this ends in a full page load.
    await Promise.all([page.waitForURL(/\/[^/]+\//), this.submitOf(passwordForm).click()]);
  }

  async openProjectIssues(workspaceSlug: string, projectId: string): Promise<void> {
    await this.page.goto(`/${workspaceSlug}/projects/${projectId}/issues`);
    await this.page.getByTestId(WEB_TEST_IDS.issueName).first().waitFor({ timeout: 120_000 });
  }

  async visibleIssueNames(): Promise<string[]> {
    return this.page.getByTestId(WEB_TEST_IDS.issueName).allTextContents();
  }
}
