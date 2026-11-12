// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-442): instance-admin general settings and
// email (SMTP) settings. Rows ADM-013 (rename the instance), ADM-014
// (admin email read-only), ADM-015 (instance ID read-only), ADM-016
// (telemetry toggle, persists on save), ADM-017 (general save commits
// name plus telemetry; silent-failure quirk), ADM-018 (master email
// enable/disable toggle), ADM-019 (SMTP config form), ADM-020 (the
// TLS/SSL/none selector over two flags), ADM-021 (send-test-email
// modal) and ADM-022 (email loading skeletons).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the general form always submits name and telemetry
// together and reports success with a notice; the two read-only fields
// never ride along. Clearing the name blocks the submit with no visible
// feedback at all (bug: NEWFRONT-475). The telemetry flip alone writes
// nothing; only the shared save persists it. A failed general save (and
// a failed config save alike) surfaces no notice — the failure lands in
// the console only. The master email toggle derives from the stored
// flag; switching off calls the disable endpoint and hides the form,
// while switching on only reveals it. A failed disable still reports
// success and hides the form while the server stays enabled (bug:
// NEWFRONT-476). The config save forces the enabled flag on, stays shut
// until the form is both valid and touched, and leaves the test action
// gated on validity alone. The security picker maps three choices onto
// two flags with TLS winning ties, though flipping it alone never dirties
// the form so the save stays shut (bug: NEWFRONT-478). The test modal runs compose, success
// and failure steps and resets on close, but its check POST carries only
// the receiver address, so the server grades the stored settings rather
// than the entered ones (bug: NEWFRONT-477); the new app also sends the
// entered settings. While the configuration
// loads, a pill stands in for the toggle; the multi-row form skeleton
// shows only once the toggle is on with no configuration behind it.
//
// Target behavior (NEWFRONT-475/476/477/478): the scenarios below run
// the intended rules on apps/web_new — the empty name names the field
// inline, a failed disable reports the failure and rolls the toggle
// back, the security picker dirties the form, and the test action stays
// open on unsaved edits while the check grades the entered settings.
// The oracle keeps the pinned old outcomes.
import { test, expect } from "../fixtures";
import {
  serverAdminEmailCheck,
  serverAdminGetConfigurations,
  serverAdminInstanceInfo,
  serverAdminListAdmins,
  serverAdminSession,
  serverAdminUpdateConfigurations,
  serverAdminUpdateInstance,
  uniqueEmail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** SMTP configuration keys this spec snapshots and restores. */
const SMTP_KEYS = [
  "EMAIL_HOST",
  "EMAIL_PORT",
  "EMAIL_HOST_USER",
  "EMAIL_HOST_PASSWORD",
  "EMAIL_USE_TLS",
  "EMAIL_USE_SSL",
  "EMAIL_FROM",
  "ENABLE_SMTP",
] as const;

/** Local sleep for proving an absence (no write, no notice) over a window. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Fully-disabled SMTP configuration (the seed shape). Tests that need
 * the off state establish it explicitly instead of assuming the seed.
 */
const DISABLED_SMTP: Record<string, string> = {
  EMAIL_HOST: "",
  EMAIL_PORT: "587",
  EMAIL_HOST_USER: "",
  EMAIL_HOST_PASSWORD: "",
  EMAIL_USE_TLS: "1",
  EMAIL_USE_SSL: "0",
  EMAIL_FROM: "",
  ENABLE_SMTP: "0",
};

/** Snapshot the SMTP rows this spec may rewrite, for teardown restore. */
async function snapshotSmtp(session: string): Promise<Record<string, string>> {
  const all = await serverAdminGetConfigurations(session);
  return Object.fromEntries(SMTP_KEYS.map((key) => [key, all[key] ?? ""]));
}

/** Restore SMTP rows and prove the restore landed (keeps retries convergent). */
async function restoreSmtp(session: string, snap: Record<string, string>): Promise<void> {
  await serverAdminUpdateConfigurations(session, snap);
  const after = await serverAdminGetConfigurations(session);
  for (const key of SMTP_KEYS) {
    expect(after[key]).toBe(snap[key]);
  }
}

/** Snapshot the general settings this spec may rewrite. */
async function snapshotGeneral(): Promise<{ instanceName: string; telemetryEnabled: boolean }> {
  const info = await serverAdminInstanceInfo();
  return { instanceName: info.instanceName, telemetryEnabled: info.telemetryEnabled };
}

/** Restore general settings and prove the restore landed. */
async function restoreGeneral(
  session: string,
  snap: { instanceName: string; telemetryEnabled: boolean }
): Promise<void> {
  await serverAdminUpdateInstance(session, snap);
  const after = await serverAdminInstanceInfo();
  expect(after.instanceName).toBe(snap.instanceName);
  expect(after.telemetryEnabled).toBe(snap.telemetryEnabled);
}

/** Open general settings signed in, waiting for the form to render. */
async function openGeneral(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/general/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect
    .poll(
      async () => {
        try {
          return (await driver.adminGeneralFacts()).email;
        } catch {
          return "";
        }
      },
      { timeout: 15_000 }
    )
    .not.toBe("");
}

/** Open the email page signed in, waiting for the header to settle. */
async function openEmail(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/email/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminEmailFacts()).toggleVisible, { timeout: 15_000 }).toBe(true);
}

test(
  specTitle(["ADM-013"], "rename persists with a success notice"),
  { tag: specTags(["ADM-013"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotGeneral();
    const renamed = "Parity Rename Probe";
    try {
      await openGeneral(driver, seed);
      expect((await driver.adminGeneralFacts()).name).toBe(before.instanceName);

      await driver.adminStartInstancePatchSpy();
      await driver.adminGeneralFillName(renamed);
      await driver.adminGeneralSave();
      await expect.poll(async () => (await driver.adminInstancePatchBodies()).length, { timeout: 15_000 }).toBe(1);
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
      expect(await driver.adminLastToast()).toEqual({
        title: "Success",
        message: "Settings updated successfully",
      });

      // The server kept the rename; a fresh load shows it in the field.
      expect((await serverAdminInstanceInfo()).instanceName).toBe(renamed);
      await driver.adminOpenPath("/general/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminGeneralFacts()).name, { timeout: 15_000 }).toBe(renamed);
    } finally {
      await restoreGeneral(session, before);
    }
  }
);

test(
  specTitle(["ADM-013"], "bug: NEWFRONT-475 empty name blocks the save with no message"),
  { tag: specTags(["ADM-013"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const before = await snapshotGeneral();
    await openGeneral(driver, seed);

    await driver.adminStartInstancePatchSpy();
    await driver.adminStartConsoleSpy();
    await driver.adminGeneralFillName("");
    await driver.adminGeneralSave();
    // The blocked submit sends nothing and reports nothing: no write,
    // no notice, and the button keeps its idle label.
    await sleep(2000);
    expect(await driver.adminInstancePatchBodies()).toEqual([]);
    expect(await driver.adminLastToast()).toBeNull();
    expect((await driver.adminGeneralFacts()).saveLabel).toBe("Save changes");
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-475): the blocked save names the required
      // field inline instead of failing silently.
      expect((await driver.adminGeneralFacts()).nameError).toBe("Instance name is required.");
      // A real value clears the message and lets the save through.
      await driver.adminGeneralFillName(before.instanceName);
      await expect.poll(async () => (await driver.adminGeneralFacts()).nameError, { timeout: 15_000 }).toBeNull();
    }

    // The server kept the old name.
    expect((await serverAdminInstanceInfo()).instanceName).toBe(before.instanceName);
  }
);

test(
  specTitle(["ADM-014", "ADM-015"], "admin email and instance ID render read-only and never submit"),
  { tag: specTags(["ADM-014", "ADM-015"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotGeneral();
    try {
      await openGeneral(driver, seed);
      const facts = await driver.adminGeneralFacts();
      expect(facts.nameDisabled).toBe(false);
      expect(facts.emailDisabled).toBe(true);
      expect(facts.instanceIdDisabled).toBe(true);

      // The read-only fields mirror the server: the first admin's
      // address and the instance identifier.
      const info = await serverAdminInstanceInfo();
      expect(facts.email).toBe((await serverAdminListAdmins(session))[0]);
      expect(facts.instanceId).toBe(info.instanceId);

      // A save carries only the two writable fields.
      await driver.adminStartInstancePatchSpy();
      await driver.adminGeneralFillName("Parity Readonly Probe");
      await driver.adminGeneralSave();
      await expect.poll(async () => (await driver.adminInstancePatchBodies()).length, { timeout: 15_000 }).toBe(1);
      const bodies = await driver.adminInstancePatchBodies();
      expect(Object.keys((bodies[0] ?? {}) as Record<string, unknown>).sort()).toEqual([
        "instance_name",
        "is_telemetry_enabled",
      ]);
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
    } finally {
      await restoreGeneral(session, before);
    }
  }
);

test(
  specTitle(["ADM-016"], "telemetry flips locally and persists only on save"),
  { tag: specTags(["ADM-016"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotGeneral();
    try {
      await openGeneral(driver, seed);
      const facts = await driver.adminGeneralFacts();
      expect(facts.telemetryOn).toBe(before.telemetryEnabled);
      expect(facts.telemetryDisabled).toBe(false);
      expect(facts.telemetryPolicyHref).toBe("https://github.com/The-AI-Republic/pi-dash#readme");
      expect(facts.telemetryPolicyTarget).toBe("_blank");

      await driver.adminStartInstancePatchSpy();
      // The flip alone writes nothing anywhere.
      await driver.adminGeneralSetTelemetry(!before.telemetryEnabled);
      expect((await driver.adminGeneralFacts()).telemetryOn).toBe(!before.telemetryEnabled);
      await sleep(1500);
      expect(await driver.adminInstancePatchBodies()).toEqual([]);
      expect((await serverAdminInstanceInfo()).telemetryEnabled).toBe(before.telemetryEnabled);

      // Only the shared save persists the flip.
      await driver.adminGeneralSave();
      await expect.poll(async () => (await driver.adminInstancePatchBodies()).length, { timeout: 15_000 }).toBe(1);
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
      expect((await serverAdminInstanceInfo()).telemetryEnabled).toBe(!before.telemetryEnabled);
    } finally {
      await restoreGeneral(session, before);
    }
  }
);

test(
  specTitle(["ADM-016", "ADM-017"], "in-flight save swaps the label and shuts the switch off"),
  { tag: specTags(["ADM-016", "ADM-017"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotGeneral();
    try {
      await openGeneral(driver, seed);
      await driver.adminStartInstancePatchSpy();
      await driver.adminDelayInstancePatch(3000);
      try {
        await driver.adminGeneralFillName("Parity Busy Probe");
        await driver.adminGeneralSave();
        // While the write hangs, the button carries the busy label and
        // the telemetry switch is disabled.
        await expect.poll(async () => (await driver.adminGeneralFacts()).saveLabel, { timeout: 15_000 }).toBe("Saving");
        expect((await driver.adminGeneralFacts()).telemetryDisabled).toBe(true);
        // The held write still lands once the delay lifts.
        await expect
          .poll(async () => (await driver.adminGeneralFacts()).saveLabel, { timeout: 15_000 })
          .toBe("Save changes");
        expect((await driver.adminGeneralFacts()).telemetryDisabled).toBe(false);
        await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect((await serverAdminInstanceInfo()).instanceName).toBe("Parity Busy Probe");
      } finally {
        await driver.adminClearInstancePatchFault();
      }
    } finally {
      await restoreGeneral(session, before);
    }
  }
);

test(
  specTitle(["ADM-017"], "one save commits name and telemetry together"),
  { tag: specTags(["ADM-017"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotGeneral();
    const renamed = "Parity Atomic Probe";
    try {
      await openGeneral(driver, seed);
      await driver.adminStartInstancePatchSpy();
      await driver.adminGeneralFillName(renamed);
      await driver.adminGeneralSetTelemetry(!before.telemetryEnabled);
      await driver.adminGeneralSave();
      await expect.poll(async () => (await driver.adminInstancePatchBodies()).length, { timeout: 15_000 }).toBe(1);
      // A single write carries both fields.
      expect(await driver.adminInstancePatchBodies()).toEqual([
        { instance_name: renamed, is_telemetry_enabled: !before.telemetryEnabled },
      ]);
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
      const after = await serverAdminInstanceInfo();
      expect(after.instanceName).toBe(renamed);
      expect(after.telemetryEnabled).toBe(!before.telemetryEnabled);
    } finally {
      await restoreGeneral(session, before);
    }
  }
);

test(
  specTitle(["ADM-017"], "failed save stays silent apart from a console error"),
  { tag: specTags(["ADM-017"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const before = await snapshotGeneral();
    await openGeneral(driver, seed);
    await driver.adminStartInstancePatchSpy();
    await driver.adminStartConsoleSpy();
    await driver.adminFailInstancePatch();
    try {
      await driver.adminGeneralFillName("Parity Failure Probe");
      await driver.adminGeneralSave();
      await expect.poll(async () => (await driver.adminInstancePatchBodies()).length, { timeout: 15_000 }).toBe(1);
      // The failure lands in the console, and the button returns to idle.
      await expect
        .poll(
          async () =>
            (await driver.adminConsoleErrors()).some((line) => line.includes("Error updating the instance info")),
          { timeout: 15_000 }
        )
        .toBe(true);
      await expect
        .poll(async () => (await driver.adminGeneralFacts()).saveLabel, { timeout: 15_000 })
        .toBe("Save changes");
      // But no notice ever shows, and the server is untouched.
      await sleep(2000);
      expect(await driver.adminLastToast()).toBeNull();
      const after = await serverAdminInstanceInfo();
      expect(after.instanceName).toBe(before.instanceName);
      expect(after.telemetryEnabled).toBe(before.telemetryEnabled);
    } finally {
      await driver.adminClearInstancePatchFault();
    }
  }
);

test(
  specTitle(["ADM-018"], "master toggle disables through the endpoint and re-enables locally"),
  { tag: specTags(["ADM-018"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      // The toggle derives its initial state from the stored flag.
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      let shell = await driver.adminEmailFacts();
      expect(shell.toggleOn).toBe(true);
      expect(shell.formVisible).toBe(true);

      // Switching off calls the disable endpoint exactly once, reports
      // it, and hides the form.
      await driver.adminStartEmailDisableSpy();
      await driver.adminStartConfigPatchSpy();
      await driver.adminEmailSetEnabled(false);
      await expect.poll(async () => driver.adminEmailDisableCount(), { timeout: 15_000 }).toBe(1);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(false);
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
      expect(await driver.adminLastToast()).toEqual({
        title: "Email feature disabled",
        message: "Email feature has been disabled",
      });

      // The server blanked the mail keys and stood the flag down but
      // kept the transport-security pair.
      const disabled = await serverAdminGetConfigurations(session);
      expect(disabled["ENABLE_SMTP"]).toBe("0");
      expect(disabled["EMAIL_HOST"]).toBe("");
      expect(disabled["EMAIL_PORT"]).toBe("");
      expect(disabled["EMAIL_HOST_USER"]).toBe("");
      expect(disabled["EMAIL_HOST_PASSWORD"]).toBe("");
      expect(disabled["EMAIL_FROM"]).toBe("");
      expect(disabled["EMAIL_USE_TLS"]).toBe(before["EMAIL_USE_TLS"]);
      expect(disabled["EMAIL_USE_SSL"]).toBe(before["EMAIL_USE_SSL"]);

      // Switching back on reveals the form with the blanked values and
      // calls nothing.
      await driver.adminEmailSetEnabled(true);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      await sleep(1500);
      expect(await driver.adminEmailDisableCount()).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([]);
      const form = await driver.adminEmailFormFacts();
      expect(form.values.host).toBe("");
      expect(form.values.port).toBe("");
      expect(form.values.from).toBe("");
      shell = await driver.adminEmailFacts();
      expect(shell.toggleOn).toBe(true);
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-018"], "bug: NEWFRONT-476 failed disable still reports success"),
  { tag: specTags(["ADM-018"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      expect((await driver.adminEmailFacts()).toggleOn).toBe(true);

      await driver.adminStartEmailDisableSpy();
      await driver.adminStartConsoleSpy();
      await driver.adminFailEmailDisable();
      try {
        await driver.adminEmailSetEnabled(false);
        await expect.poll(async () => driver.adminEmailDisableCount(), { timeout: 15_000 }).toBe(1);
        await expect
          .poll(
            async () => (await driver.adminConsoleErrors()).some((line) => line.includes("Error disabling the email")),
            { timeout: 15_000 }
          )
          .toBe(true);
        if (driver.target === "web_new") {
          // Intended (NEWFRONT-476): the failure is reported as a
          // failure, the toggle rolls back on, and the form stays.
          await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
          const notice = await driver.adminLastToast();
          expect(notice?.title ?? "").toContain("Could not disable email");
          expect(notice?.message ?? "").toContain("left unchanged");
          await expect.poll(async () => (await driver.adminEmailFacts()).toggleOn, { timeout: 15_000 }).toBe(true);
          expect((await driver.adminEmailFacts()).formVisible).toBe(true);
        } else {
          // The failure reports success: the notice, the hidden form and
          // the off toggle all claim the feature is down.
          await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(false);
          await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
          expect(await driver.adminLastToast()).toEqual({
            title: "Email feature disabled",
            message: "Email feature has been disabled",
          });
        }

        // But the server still holds the enabled configuration.
        const kept = await serverAdminGetConfigurations(session);
        expect(kept["ENABLE_SMTP"]).toBe("1");
        expect(kept["EMAIL_HOST"]).toBe("parity-smtp.invalid");

        // Flipping back on reveals the restored values.
        await driver.adminEmailSetEnabled(true);
        await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
        const form = await driver.adminEmailFormFacts();
        expect(form.values.host).toBe("parity-smtp.invalid");
        expect(form.values.from).toBe("parity-smtp@example.com");
      } finally {
        await driver.adminClearEmailDisableFault();
      }
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-019"], "SMTP form validates, gates save, and persists with SMTP forced on"),
  { tag: specTags(["ADM-019"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    const host = "parity-smtp.invalid";
    const from = "parity-smtp@example.com";
    try {
      // Establish the disabled starting point explicitly (never assume seed).
      await serverAdminUpdateConfigurations(session, DISABLED_SMTP);
      await openEmail(driver, seed);
      expect((await driver.adminEmailFacts()).toggleOn).toBe(false);
      await driver.adminEmailSetEnabled(true);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);

      // The fresh form shows the established values with the required
      // fields empty, so both actions stay shut.
      let form = await driver.adminEmailFormFacts();
      expect(form.values).toEqual({ host: "", port: "587", from: "", user: "", passwordFilled: false });
      expect(form.passwordShown).toBe(false);
      expect(form.saveDisabled).toBe(true);
      expect(form.testEmailDisabled).toBe(true);

      // One required field is not enough; both open both actions.
      await driver.adminEmailFill({ host });
      expect((await driver.adminEmailFormFacts()).saveDisabled).toBe(true);
      expect((await driver.adminEmailFormFacts()).testEmailDisabled).toBe(true);
      await driver.adminEmailFill({ from, user: "parityuser", password: "paritysecret" });
      form = await driver.adminEmailFormFacts();
      expect(form.values.passwordFilled).toBe(true);
      expect(form.saveDisabled).toBe(false);
      // The check grades the entered settings, so the test action opens
      // on validity alone and never asks for a save first.
      expect(form.testEmailDisabled).toBe(false);
      expect(form.saveFirstHint).toBeNull();

      // The password toggle flips the field without touching the value.
      await driver.adminEmailTogglePassword();
      expect((await driver.adminEmailFormFacts()).passwordShown).toBe(true);
      await driver.adminEmailTogglePassword();
      expect((await driver.adminEmailFormFacts()).passwordShown).toBe(false);
      expect((await driver.adminEmailFormFacts()).values.passwordFilled).toBe(true);

      // Saving persists the keys and forces the enabled flag on even
      // though the form opened while it stood at off.
      await driver.adminStartConfigPatchSpy();
      await driver.adminEmailSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      const bodies = await driver.adminConfigPatchBodies();
      expect(((bodies[0] ?? {}) as Record<string, unknown>)["ENABLE_SMTP"]).toBe("1");
      await expect.poll(async () => driver.adminLastToast(), { timeout: 15_000 }).not.toBeNull();
      expect(await driver.adminLastToast()).toEqual({
        title: "Success",
        message: "Email Settings updated successfully",
      });
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["EMAIL_HOST"]).toBe(host);
      expect(saved["EMAIL_FROM"]).toBe(from);
      expect(saved["EMAIL_HOST_USER"]).toBe("parityuser");
      expect(saved["EMAIL_HOST_PASSWORD"]).toBe("paritysecret");
      expect(saved["ENABLE_SMTP"]).toBe("1");

      // A fresh load of the saved, valid form gates save on dirtiness
      // while the test action stays open.
      await driver.adminOpenPath("/email/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      form = await driver.adminEmailFormFacts();
      expect(form.values.host).toBe(host);
      expect(form.saveDisabled).toBe(true);
      expect(form.testEmailDisabled).toBe(false);
      expect(form.saveFirstHint).toBeNull();
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-019"], "failed config save stays silent apart from a console error"),
  { tag: specTags(["ADM-019"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await openEmail(driver, seed);
      await driver.adminEmailSetEnabled(true);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      await driver.adminEmailFill({ host: "parity-smtp.invalid", from: "parity-smtp@example.com" });
      expect((await driver.adminEmailFormFacts()).saveDisabled).toBe(false);

      await driver.adminStartConfigPatchSpy();
      await driver.adminStartConsoleSpy();
      await driver.adminFailConfigPatch();
      try {
        await driver.adminEmailSave();
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(
            async () =>
              (await driver.adminConsoleErrors()).some((line) =>
                line.includes("Error updating the instance configurations")
              ),
            { timeout: 15_000 }
          )
          .toBe(true);
        await expect
          .poll(async () => (await driver.adminEmailFormFacts()).saveLabel, { timeout: 15_000 })
          .toBe("Save changes");
        await sleep(2000);
        expect(await driver.adminLastToast()).toBeNull();

        // The server kept the previous configuration.
        const kept = await serverAdminGetConfigurations(session);
        for (const key of SMTP_KEYS) {
          expect(kept[key]).toBe(before[key]);
        }
      } finally {
        await driver.adminClearConfigPatchFault();
      }
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-020"], "bug: NEWFRONT-478 security-only change leaves save shut"),
  { tag: specTags(["ADM-020"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        EMAIL_USE_TLS: "1",
        EMAIL_USE_SSL: "0",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);

      await driver.adminStartConfigPatchSpy();
      await driver.adminEmailSetSecurity("SSL");
      const form = await driver.adminEmailFormFacts();
      expect(form.security).toBe("SSL");
      if (driver.target === "web_new") {
        // Intended (NEWFRONT-478): the picker dirties the form like any
        // other control, so the save opens and persists the flag pair.
        expect(form.saveDisabled).toBe(false);
        await driver.adminEmailSave();
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        const kept = await serverAdminGetConfigurations(session);
        expect(kept["EMAIL_USE_TLS"]).toBe("0");
        expect(kept["EMAIL_USE_SSL"]).toBe("1");
      } else {
        // Flipping only the security choice never dirties the form, so
        // the save stays shut and the flip can never persist on its own.
        expect(form.saveDisabled).toBe(true);
        await sleep(1500);
        expect(await driver.adminConfigPatchBodies()).toEqual([]);
        const kept = await serverAdminGetConfigurations(session);
        expect(kept["EMAIL_USE_TLS"]).toBe("1");
        expect(kept["EMAIL_USE_SSL"]).toBe("0");
      }
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-020"], "security choice maps onto the TLS/SSL flags"),
  { tag: specTags(["ADM-020"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        EMAIL_USE_TLS: "1",
        EMAIL_USE_SSL: "0",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      expect((await driver.adminEmailFormFacts()).security).toBe("TLS");

      await driver.adminStartConfigPatchSpy();
      // Each choice persists as exactly one flag on (or neither). The
      // host edit alongside dirties the form (bug NEWFRONT-478 pins the
      // security-only case above).
      for (const [choice, tls, ssl, host] of [
        ["SSL", "0", "1", "parity-smtp-ssl.invalid"],
        ["none", "0", "0", "parity-smtp-none.invalid"],
        ["TLS", "1", "0", "parity-smtp-tls.invalid"],
      ] as const) {
        await driver.adminEmailSetSecurity(choice);
        expect((await driver.adminEmailFormFacts()).security).toBe(choice);
        await driver.adminEmailFill({ host });
        const seen = (await driver.adminConfigPatchBodies()).length;
        await driver.adminEmailSave();
        await expect
          .poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 })
          .toBe(seen + 1);
        const saved = await serverAdminGetConfigurations(session);
        expect(saved["EMAIL_USE_TLS"]).toBe(tls);
        expect(saved["EMAIL_USE_SSL"]).toBe(ssl);
        expect(saved["EMAIL_HOST"]).toBe(host);
      }

      // A fresh load derives the stored choice; when both flags stand
      // on, TLS wins the display.
      await driver.adminOpenPath("/email/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      expect((await driver.adminEmailFormFacts()).security).toBe("TLS");
      await serverAdminUpdateConfigurations(session, { EMAIL_USE_TLS: "1", EMAIL_USE_SSL: "1" });
      await driver.adminOpenPath("/email/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      expect((await driver.adminEmailFormFacts()).security).toBe("TLS");
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-021"], "modal sends the check, succeeds, and resets on close"),
  { tag: specTags(["ADM-021"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    const receiver = uniqueEmail("parity-adm-test");
    try {
      // Pin every SMTP key so the form loads a fully known state.
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_HOST_USER: "",
        EMAIL_HOST_PASSWORD: "",
        EMAIL_USE_TLS: "1",
        EMAIL_USE_SSL: "0",
        EMAIL_FROM: "parity-smtp@example.com",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      // The saved, valid form leaves the test action open.
      expect((await driver.adminEmailFormFacts()).testEmailDisabled).toBe(false);

      await driver.adminTestEmailOpen();
      let modal = await driver.adminTestEmailFacts();
      expect(modal.open).toBe(true);
      expect(modal.step).toBe("compose");
      expect(modal.receiver).toBe("");
      expect(modal.heading).toBe("Send test email");
      expect(modal.sendLabel).toBe("Send email");
      expect(modal.dismissLabel).toBe("Cancel");

      await driver.adminStartEmailCheckSpy();
      await driver.adminTestEmailFillReceiver(receiver);
      await driver.adminTestEmailSend();
      await expect.poll(async () => (await driver.adminTestEmailFacts()).step, { timeout: 90_000 }).toBe("success");
      modal = await driver.adminTestEmailFacts();
      expect(modal.heading).toBe("Email send");
      expect(modal.bodyText).toContain(receiver);
      expect(modal.bodyText).toContain("spam");
      expect(modal.sendLabel).toBeNull();
      expect(modal.dismissLabel).toBe("Close");

      if (driver.target === "web_new") {
        // The check POST carries the receiver plus the form's current
        // settings; the blank password is omitted so the server grades
        // the stored secret.
        expect(await driver.adminEmailCheckBodies()).toEqual([
          {
            receiver_email: receiver,
            EMAIL_HOST: "parity-smtp.invalid",
            EMAIL_PORT: "587",
            EMAIL_HOST_USER: "",
            EMAIL_USE_TLS: "1",
            EMAIL_USE_SSL: "0",
            EMAIL_FROM: "parity-smtp@example.com",
          },
        ]);
      } else {
        // The check POST carries only the receiver address.
        expect(await driver.adminEmailCheckBodies()).toEqual([{ receiver_email: receiver }]);
      }
      const direct = await serverAdminEmailCheck(session, receiver);
      expect(direct.status).toBe(200);

      // Closing resets the modal: reopening composes fresh.
      await driver.adminTestEmailClose();
      await expect.poll(async () => (await driver.adminTestEmailFacts()).open, { timeout: 15_000 }).toBe(false);
      await driver.adminTestEmailOpen();
      modal = await driver.adminTestEmailFacts();
      expect(modal.step).toBe("compose");
      expect(modal.receiver).toBe("");
      expect(modal.dismissLabel).toBe("Cancel");
      await driver.adminTestEmailClose();
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-021"], "empty receiver fails with the server message"),
  { tag: specTags(["ADM-021"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        ENABLE_SMTP: "1",
      });
      await openEmail(driver, seed);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);

      await driver.adminTestEmailOpen();
      await driver.adminTestEmailSend();
      // The check posts to the scratch SMTP host; under host load the
      // round trip can outlast a tight poll, so the budget is the
      // suite's own rather than the default expect timeout.
      await expect.poll(async () => (await driver.adminTestEmailFacts()).step, { timeout: 90_000 }).toBe("failed");
      const modal = await driver.adminTestEmailFacts();
      expect(modal.heading).toBe("Failed");
      expect(modal.dismissLabel).toBe("Close");
      // The failure step shows the server's own error message.
      const direct = await serverAdminEmailCheck(session, "");
      expect(direct.status).toBe(400);
      expect(modal.bodyText).toBe((direct.body as { error?: unknown })?.error);

      await driver.adminTestEmailClose();
      await expect.poll(async () => (await driver.adminTestEmailFacts()).open, { timeout: 15_000 }).toBe(false);
    } finally {
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-021"], "bug: NEWFRONT-477 check runs against stored settings, not entered ones"),
  { tag: specTags(["ADM-021"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const receiver = uniqueEmail("parity-adm-stored");
    const stored = await snapshotSmtp(session);
    // Establish a stored configuration that differs from the values the
    // form will carry, so a rerun never starts from the same state.
    await serverAdminUpdateConfigurations(session, {
      ...DISABLED_SMTP,
      EMAIL_HOST: "stored-smtp.invalid",
      EMAIL_FROM: "stored@example.com",
      ENABLE_SMTP: "1",
    });
    try {
      await openEmail(driver, seed);
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      await driver.adminEmailFill({ host: "entered-smtp.invalid", from: "entered@example.com" });
      if (driver.target === "web_new") {
        // Intended (NEWFRONT-477): the check grades the entered
        // settings, so unsaved edits leave the test action open with
        // no save-first hint.
        const dirty = await driver.adminEmailFormFacts();
        expect(dirty.testEmailDisabled).toBe(false);
        expect(dirty.saveFirstHint).toBeNull();
      } else {
        expect((await driver.adminEmailFormFacts()).testEmailDisabled).toBe(false);
      }

      await driver.adminTestEmailOpen();
      await driver.adminStartEmailCheckSpy();
      await driver.adminTestEmailFillReceiver(receiver);
      await driver.adminTestEmailSend();
      await expect.poll(async () => (await driver.adminTestEmailFacts()).step, { timeout: 30_000 }).toBe("success");
      if (driver.target === "web_new") {
        // The body carries the entered settings — the host and sender
        // the form holds, not the stored ones — with the rest of the
        // form's current values alongside.
        expect(await driver.adminEmailCheckBodies()).toEqual([
          {
            receiver_email: receiver,
            EMAIL_HOST: "entered-smtp.invalid",
            EMAIL_PORT: "587",
            EMAIL_HOST_USER: "",
            EMAIL_USE_TLS: "1",
            EMAIL_USE_SSL: "0",
            EMAIL_FROM: "entered@example.com",
          },
        ]);
      } else {
        // The body carries only the receiver: the server grades its own
        // stored settings rather than anything the form could add.
        expect(await driver.adminEmailCheckBodies()).toEqual([{ receiver_email: receiver }]);
      }
      const direct = await serverAdminEmailCheck(session, receiver);
      expect(direct.status).toBe(200);
      await driver.adminTestEmailClose();
    } finally {
      await restoreSmtp(session, stored);
    }
  }
);

test(
  specTitle(["ADM-022"], "loading shows the pill skeleton, never beside the toggle"),
  { tag: specTags(["ADM-022"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await driver.adminEnsureSignedIn(seed.email, seed.password);

      await test.step("stored-disabled load settles on the off toggle", async () => {
        await serverAdminUpdateConfigurations(session, DISABLED_SMTP);
        await driver.adminDelayConfigFetch(4000);
        await driver.adminOpenPath("/email/");
        // The pill stands in with neither the toggle nor the form beside it.
        await expect
          .poll(
            async () => {
              const shell = await driver.adminEmailFacts();
              return shell.headerSkeletonVisible && !shell.toggleVisible && !shell.formVisible
                ? "skeleton-only"
                : "waiting";
            },
            { timeout: 15_000 }
          )
          .toBe("skeleton-only");
        await expect.poll(async () => (await driver.adminEmailFacts()).toggleVisible, { timeout: 15_000 }).toBe(true);
        const shell = await driver.adminEmailFacts();
        expect(shell.toggleOn).toBe(false);
        expect(shell.formVisible).toBe(false);
        expect(shell.headerSkeletonVisible).toBe(false);
        expect(shell.formSkeletonVisible).toBe(false);
        await driver.adminClearConfigFetchFault();
      });

      await test.step("stored-enabled load settles on the toggle plus the form", async () => {
        await serverAdminUpdateConfigurations(session, {
          EMAIL_HOST: "parity-smtp.invalid",
          EMAIL_PORT: "587",
          EMAIL_FROM: "parity-smtp@example.com",
          ENABLE_SMTP: "1",
        });
        await driver.adminDelayConfigFetch(4000);
        await driver.adminOpenPath("/email/");
        await expect
          .poll(
            async () => {
              const shell = await driver.adminEmailFacts();
              return shell.headerSkeletonVisible && !shell.toggleVisible && !shell.formVisible
                ? "skeleton-only"
                : "waiting";
            },
            { timeout: 15_000 }
          )
          .toBe("skeleton-only");
        await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
        const shell = await driver.adminEmailFacts();
        expect(shell.toggleOn).toBe(true);
        expect(shell.headerSkeletonVisible).toBe(false);
        expect(shell.formSkeletonVisible).toBe(false);
        await driver.adminClearConfigFetchFault();
      });
    } finally {
      await driver.adminClearConfigFetchFault();
      await restoreSmtp(session, before);
    }
  }
);

test(
  specTitle(["ADM-022"], "failed load with email on shows the form skeleton"),
  { tag: specTags(["ADM-022"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotSmtp(session);
    try {
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      await driver.adminFailConfigFetch();
      try {
        await driver.adminOpenPath("/email/");
        // The header settles (the toggle reads off, never having learned
        // otherwise) while the failed read leaves no configuration.
        await expect
          .poll(async () => (await driver.adminEmailFacts()).toggleVisible, {
            timeout: driver.target === "web_new" ? 90_000 : 15_000,
          })
          .toBe(true);
        expect((await driver.adminEmailFacts()).toggleOn).toBe(false);

        // Switching on with no configuration shows the multi-row
        // skeleton instead of the form.
        await driver.adminEmailSetEnabled(true);
        await expect
          .poll(
            async () => {
              const shell = await driver.adminEmailFacts();
              return shell.formSkeletonVisible && !shell.formVisible && !shell.headerSkeletonVisible
                ? `skeleton-${shell.formSkeletonRows}`
                : "waiting";
            },
            { timeout: 15_000 }
          )
          .toBe("skeleton-5");
      } finally {
        await driver.adminClearConfigFetchFault();
      }

      // Once the read succeeds, the form replaces the skeleton.
      await serverAdminUpdateConfigurations(session, {
        EMAIL_HOST: "parity-smtp.invalid",
        EMAIL_PORT: "587",
        EMAIL_FROM: "parity-smtp@example.com",
        ENABLE_SMTP: "1",
      });
      await driver.adminOpenPath("/email/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminEmailFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      const shell = await driver.adminEmailFacts();
      expect(shell.formSkeletonVisible).toBe(false);
      expect(shell.headerSkeletonVisible).toBe(false);
    } finally {
      await restoreSmtp(session, before);
    }
  }
);
