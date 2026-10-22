// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-426): profile-settings account tabs. Rows
// PROF-011 (name edits with validation, locked email), PROF-012 (avatar
// upload dialog plus remove-to-placeholder), PROF-013 (email change gated
// on instance mail), PROF-014 (self-service deactivation, self-managed
// only) and PROF-015 (password change with strength, confirmation and
// reveal controls).
//
// Observed behavior notes (this checkout): first and display names are
// required with per-field inline errors; last name is labeled optional but
// an emptied value is rejected as required (bug NEWFRONT-432, intended
// savable). The email input is always disabled and the change-email entry
// appears only when the instance reports mail configured. The avatar
// control opens an upload dialog that rejects non-images and over-5MB
// files, uploads with an auto-save, and removes back to the placeholder
// with its own feedback. The general tab still carries a cover-image
// control (removal target per main#523/#574, unmerged): pinned as present
// with the avatar steps and reworked once those PRs merge. The email flow
// verifies a mailed code in a second step (wrong codes fail inline,
// success signs the user out); deactivation confirms through a dialog,
// deletes the account and lands on the signed-out entry — both success
// toasts fire before their sign-out navigation destroys them, so only the
// durable outcomes are pinned. This checkout is self-managed, so only the
// deactivation present state is pinned; the managed/cloud absence is
// unobservable here. The security tab skips the current password for
// provider-created accounts, gates submit on strength, difference and
// confirmation match with focus-aware note timing, reveals each field,
// maps wrong-current to a toast and weak guesses to a field error, and
// resets the form on success.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  mintMagicCode,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverInstanceFlags,
  serverMagicSignUp,
  serverMintEmailUpdateCode,
  serverPatchUser,
  serverProvisionWorkspaceMember,
  serverSettingsUserRecord,
  serverUserActive,
  serverWorkspaceMembers,
  setSmtpConfigured,
  signInSession,
  uniqueEmail,
  userFacts,
  type AuthedSession,
  type ParitySeedFacts,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver } from "../drivers/parity-driver";

const avatarUploadPng = fileURLToPath(new URL("./fixtures/avatar-upload.png", import.meta.url));

type ProvisionedMember = {
  userId: string;
  session: string;
  email: string;
  password: string;
  authed: AuthedSession;
};

/** Provision a workspace member with onboarding done, ready for browser auth. */
async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string
): Promise<ProvisionedMember> {
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
  await serverCompleteOnboarding(member.session);
  // Fresh signups carry empty names, which the required first/display
  // fields reject; seed known-good originals so every save can submit.
  const seeded = await serverPatchUser(member.session, {
    first_name: "Original",
    last_name: "Member",
    display_name: "original-member",
  });
  if (!seeded.ok) throw new Error(`[parity] name seeding failed with HTTP ${seeded.status}.`);
  const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(member.session)?.[1] ?? "";
  const authed: AuthedSession = {
    email: member.email,
    password: member.password,
    userId: member.userId,
    cookie: member.session,
    csrfToken,
    apiBase: parityApiUrl(process.env),
  };
  return { ...member, authed };
}

/** Workspace membership id for cleanup, resolved by provisioned user id. */
async function membershipIdFor(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<string> {
  const rows = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
  const row = rows.find((entry) => entry.userId === userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  return row.membershipId;
}

/** Remove a provisioned member's workspace seat; lenient for finally-blocks. */
async function cleanupMember(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<void> {
  const membershipId = await membershipIdFor(seed, ownerSession, userId).catch(() => null);
  if (membershipId === null) return;
  await serverCleanupWorkspaceMember(seed.workspaceSlug, membershipId, ownerSession).catch(() => undefined);
}

/**
 * Wait for general-tab content. Opening settles on the sidebar while the
 * form still fetches the user, so content assertions need their own wait.
 */
async function awaitGeneral(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.settingsProfileGeneralSaveLabel(), { timeout: 30_000 }).toBe("Save changes");
}

/** Wait for security-tab content (same shell-first settle as general). */
async function awaitSecurity(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.settingsProfileSecurityFieldVisible("new"), { timeout: 30_000 }).toBe(true);
}

test(
  specTitle(["PROF-011"], "general tab edits names with validation and a locked email"),
  { tag: specTags(["PROF-011"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, ownerSession, "prof011-names");
    try {
      await driver.settingsProfileOpen("general", browserSessionCookies(member.authed));
      await awaitGeneral(driver);
      const original = {
        first: await driver.settingsProfileGeneralFieldValue("first_name"),
        last: await driver.settingsProfileGeneralFieldValue("last_name"),
        display: await driver.settingsProfileGeneralFieldValue("display_name"),
      };

      await test.step("the email field is locked to the account address", async () => {
        expect(await driver.settingsProfileGeneralEmailDisabled()).toBe(true);
        expect(await driver.settingsProfileGeneralFieldValue("email")).toBe(member.email);
      });

      await test.step("blank and malformed names fail inline per field", async () => {
        await driver.settingsProfileGeneralFillField("first_name", "");
        await driver.settingsProfileGeneralSave();
        expect(await driver.settingsProfileGeneralFieldError("first_name")).toBe("Please enter first name");

        await driver.settingsProfileGeneralFillField("first_name", "R2D2");
        await driver.settingsProfileGeneralSave();
        expect(await driver.settingsProfileGeneralFieldError("first_name")).toBe(
          "Names can only contain letters, spaces, hyphens, and apostrophes"
        );

        await driver.settingsProfileGeneralFillField("display_name", "has space");
        await driver.settingsProfileGeneralSave();
        expect(await driver.settingsProfileGeneralFieldError("display_name")).toBe(
          "Display name can only contain letters, numbers, periods, hyphens, and underscores"
        );

        // Failed validation submits nothing: the server record is untouched.
        const record = await serverSettingsUserRecord(member.session);
        expect(record.firstName).toBe(original.first);
        expect(record.displayName).toBe(original.display);
      });

      await test.step("valid names save with feedback and round-trip through the server", async () => {
        await driver.settingsProfileGeneralFillField("first_name", "Parity");
        await driver.settingsProfileGeneralFillField("last_name", "Probed");
        await driver.settingsProfileGeneralFillField("display_name", "Parity-Probed");
        await driver.settingsProfileGeneralSave();
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Success!",
            message: "Profile updated successfully.",
          });
        const record = await serverSettingsUserRecord(member.session);
        expect(record.firstName).toBe("Parity");
        expect(record.lastName).toBe("Probed");
        expect(record.displayName).toBe("Parity-Probed");
        expect(await driver.settingsProfileGeneralIdentity()).toEqual({
          name: "Parity Probed",
          email: member.email,
        });
      });

      await test.step("the original names restore so reruns stay green", async () => {
        await driver.settingsProfileGeneralFillField("first_name", original.first);
        await driver.settingsProfileGeneralFillField("last_name", original.last);
        await driver.settingsProfileGeneralFillField("display_name", original.display);
        await driver.settingsProfileGeneralSave();
        // Gate on the write landing first: both saves toast identically, so
        // a toast poll here can match the previous save's still-visible card
        // while this write is in flight (read pre-write state twice under
        // load). The feedback assertion after still pins the success toast.
        await expect
          .poll(() => serverSettingsUserRecord(member.session), { timeout: 30_000 })
          .toMatchObject({
            firstName: original.first,
            lastName: original.last,
            displayName: original.display,
          });
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Success!",
            message: "Profile updated successfully.",
          });
      });
    } finally {
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-011"], "bug: NEWFRONT-432 a cleared last name is rejected as required"),
  { tag: specTags(["PROF-011"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, ownerSession, "prof011-last");
    try {
      await driver.settingsProfileOpen("general", browserSessionCookies(member.authed));
      await awaitGeneral(driver);

      // Intended: last name is optional, so clearing it saves. The field
      // carries no required star yet reports a missing name.
      await driver.settingsProfileGeneralFillField("last_name", "");
      await driver.settingsProfileGeneralSave();
      if (driver.target === "web_new") {
        // Target for NEWFRONT-432 (PROF-011): an emptied last name saves.
        expect(await driver.settingsProfileGeneralFieldError("last_name")).toBeNull();
        expect((await serverSettingsUserRecord(member.session)).lastName).toBe("");
      } else {
        expect(await driver.settingsProfileGeneralFieldError("last_name")).toBe("Name is required");
        expect((await serverSettingsUserRecord(member.session)).lastName).toBe("Member");
      }
    } finally {
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-012"], "avatar uploads, rejects bad files and removes back to the placeholder"),
  { tag: specTags(["PROF-012"]) },
  async ({ driver, seed }, testInfo) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, ownerSession, "prof012-avatar");
    try {
      await driver.settingsProfileOpen("general", browserSessionCookies(member.authed));
      await awaitGeneral(driver);

      await test.step("a fresh account shows the placeholder with removal disabled", async () => {
        expect(await driver.settingsProfileAvatarPhotoVisible()).toBe(false);
        await driver.settingsProfileAvatarDialogOpen();
        expect(await driver.settingsProfileAvatarDialogVisible()).toBe(true);
        expect(await driver.settingsProfileAvatarRemoveDisabled()).toBe(true);
      });

      await test.step("non-images and oversized files are rejected inline", async () => {
        const textFile = testInfo.outputPath("not-image.txt");
        writeFileSync(textFile, "not an image");
        await driver.settingsProfileAvatarPick(textFile);
        await expect
          .poll(() => driver.settingsProfileAvatarRejection(), { timeout: 30_000 })
          .toBe("Please upload a file in a valid format.");
        const bigFile = testInfo.outputPath("too-big.png");
        writeFileSync(bigFile, Buffer.alloc(6 * 1024 * 1024, 0));
        await driver.settingsProfileAvatarPick(bigFile);
        await expect
          .poll(() => driver.settingsProfileAvatarRejection(), { timeout: 30_000 })
          .toBe("The image size cannot exceed 5 MB.");
        await driver.settingsProfileAvatarCancel();
        expect(await driver.settingsProfileAvatarDialogVisible()).toBe(false);
        expect(await driver.settingsProfileAvatarPhotoVisible()).toBe(false);
      });

      await test.step("an upload saves and the server stores the new photo", async () => {
        await driver.settingsProfileAvatarDialogOpen();
        await driver.settingsProfileAvatarUpload(avatarUploadPng);
        expect(await driver.settingsProfileAvatarDialogVisible()).toBe(false);
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Success!",
            message: "Profile updated successfully.",
          });
        expect(await driver.settingsProfileAvatarPhotoVisible()).toBe(true);
        const record = await serverSettingsUserRecord(member.session);
        expect(record.avatarUrl).not.toBe("");
      });

      await test.step("removal clears back to the placeholder on the tab and server", async () => {
        await driver.settingsProfileAvatarDialogOpen();
        expect(await driver.settingsProfileAvatarRemoveDisabled()).toBe(false);
        await driver.settingsProfileAvatarRemove();
        expect(await driver.settingsProfileAvatarDialogVisible()).toBe(false);
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Success!",
            message: "Profile picture deleted successfully.",
          });
        expect(await driver.settingsProfileAvatarPhotoVisible()).toBe(false);
        const record = await serverSettingsUserRecord(member.session);
        expect(record.avatarUrl).toBe("");
      });

      await test.step("the cover control is still present (main#523/#574 pending)", async () => {
        if (driver.target === "web_new") {
          // Target per main#565 (PROF-012): web_new ships no cover
          // control anywhere on the tab.
          expect(await driver.settingsProfileCoverControlVisible()).toBe(false);
        } else {
          // Gap: the row targets no cover control, but the removal PRs are
          // unmerged, so the control still renders. Rework this step then.
          expect(await driver.settingsProfileCoverControlVisible()).toBe(true);
        }
      });
    } finally {
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-013"], "email change stays hidden while the instance has no mail"),
  { tag: specTags(["PROF-013"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    expect((await serverInstanceFlags()).smtpConfigured).toBe(false);

    const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(ownerSession)?.[1] ?? "";
    const admin: AuthedSession = {
      email: seed.email,
      password: seed.password,
      userId: "",
      cookie: ownerSession,
      csrfToken,
      apiBase: parityApiUrl(process.env),
    };
    await driver.settingsProfileOpen("general", browserSessionCookies(admin));
    await awaitGeneral(driver);
    expect(await driver.settingsProfileChangeEmailVisible()).toBe(false);
  }
);

test(
  specTitle(["PROF-013"], "verified email change signs out after a wrong-code failure"),
  { tag: specTags(["PROF-013"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, ownerSession, "prof013-mail");
    const newEmail = uniqueEmail("parity-newmail");
    await setSmtpConfigured(true);
    try {
      await driver.settingsProfileOpen("general", browserSessionCookies(member.authed));
      await awaitGeneral(driver);

      await test.step("the entry appears once mail is configured", async () => {
        await expect.poll(() => driver.settingsProfileChangeEmailVisible(), { timeout: 30_000 }).toBe(true);
        await driver.settingsProfileChangeEmailOpen();
        expect(await driver.settingsProfileEmailModalStep()).toBe("email");
      });

      await test.step("malformed and taken addresses fail inline", async () => {
        await driver.settingsProfileEmailModalFillEmail("not-an-email");
        await driver.settingsProfileEmailModalSubmit();
        expect(await driver.settingsProfileEmailModalError("email")).toBe("Email is invalid");

        await driver.settingsProfileEmailModalFillEmail(seed.email);
        await driver.settingsProfileEmailModalSubmit();
        expect(await driver.settingsProfileEmailModalError("email")).toBe(
          "Email already exists. Please use a different one."
        );
      });

      await test.step("a fresh address advances to the locked code step", async () => {
        await driver.settingsProfileEmailModalFillEmail(newEmail);
        await driver.settingsProfileEmailModalSubmit();
        expect(await driver.settingsProfileEmailModalStep()).toBe("code");
        expect(await driver.settingsProfileEmailModalEmailDisabled()).toBe(true);
        expect(await driver.settingsProfileEmailModalHelper()).toBe("Verification code sent to your new email.");
      });

      await test.step("a wrong code fails inline without touching the account email", async () => {
        await driver.settingsProfileEmailModalFillCode("000000");
        await driver.settingsProfileEmailModalSubmit();
        expect(await driver.settingsProfileEmailModalStep()).toBe("code");
        expect(await driver.settingsProfileEmailModalError("code")).toBe("Invalid verification code");
        const record = await serverSettingsUserRecord(member.session);
        expect(record.email).toBe(member.email);
      });

      await test.step("the mailed code moves the account and signs out", async () => {
        const code = await serverMintEmailUpdateCode(member.userId, newEmail);
        await driver.settingsProfileEmailModalFillCode(code);
        await driver.settingsProfileEmailModalSubmit();
        // The success toast fires before the sign-out navigation destroys
        // it, so only the durable outcomes are pinned: dialog gone, signed
        // out, account moved.
        expect(await driver.settingsProfileEmailModalStep()).toBe("closed");
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).not.toMatch(/^\/settings/);
        await expect(signInSession(newEmail, member.password)).resolves.toContain("session-id=");
        expect((await userFacts(newEmail)).exists).toBe(true);
      });
    } finally {
      await setSmtpConfigured(false);
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-014"], "deactivation asks, cancels safely, then deletes a throwaway account"),
  { tag: specTags(["PROF-014"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    // This checkout is self-managed, so only the present state is pinned;
    // the managed/cloud absence is unobservable here (recorded in the row).
    expect((await serverInstanceFlags()).selfManaged).toBe(true);
    const member = await provisionMember(seed, ownerSession, "prof014-gone");
    try {
      await driver.settingsProfileOpen("general", browserSessionCookies(member.authed));
      await awaitGeneral(driver);

      await test.step("the danger zone warns that removal is permanent", async () => {
        expect(await driver.settingsProfileDeactivateVisible()).toBe(true);
        expect(await driver.settingsProfileDeactivateDescription()).toContain("permanently removed");
      });

      await test.step("cancelling keeps the account intact", async () => {
        await driver.settingsProfileDeactivateOpen();
        expect(await driver.settingsProfileDeactivateTitle()).toBe("Deactivate your account");
        expect(await driver.settingsProfileDeactivateWorking()).toBe(false);
        await driver.settingsProfileDeactivateCancel();
        expect(await driver.settingsProfileDeactivateTitle()).toBeNull();
        expect((await serverSettingsUserRecord(member.session)).email).toBe(member.email);
        expect(await serverUserActive(member.email)).toBe(true);
      });

      await test.step("confirming deactivates and lands on the signed-out entry", async () => {
        await driver.settingsProfileDeactivateOpen();
        await driver.settingsProfileDeactivateConfirm();
        // The success toast fires before the sign-out navigation destroys
        // it, so only the durable outcomes are pinned: signed-out landing,
        // flipped active flag, dead credentials.
        await expect.poll(() => driver.currentPath(), { timeout: 30_000 }).not.toMatch(/^\/settings/);
        expect(await serverUserActive(member.email)).toBe(false);
        await expect(signInSession(member.email, member.password)).rejects.toThrow();
      });
    } finally {
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-015"], "password change enforces strength, match and difference"),
  { tag: specTags(["PROF-015"]) },
  async ({ driver, seed }) => {
    const ownerSession = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, ownerSession, "prof015-pw");
    const current = member.password;
    const strongA = "Parity-Strong-1A";
    const strongB = "Parity-Fresh-9Z";
    try {
      await driver.settingsProfileOpen("security", browserSessionCookies(member.authed));
      await awaitSecurity(driver);

      await test.step("password accounts answer the current password with masked reveals", async () => {
        for (const field of ["current", "new", "confirm"] as const) {
          expect(await driver.settingsProfileSecurityFieldVisible(field)).toBe(true);
          expect(await driver.settingsProfileSecurityFieldType(field)).toBe("password");
          await driver.settingsProfileSecurityToggleReveal(field);
          expect(await driver.settingsProfileSecurityFieldType(field)).toBe("text");
          await driver.settingsProfileSecurityToggleReveal(field);
          expect(await driver.settingsProfileSecurityFieldType(field)).toBe("password");
        }
      });

      await test.step("a weak guess shows the strength indicator and blocks submit", async () => {
        await driver.settingsProfileSecurityFill("new", "weak");
        expect(await driver.settingsProfileSecurityStrengthVisible()).toBe(true);
        expect(await driver.settingsProfileSecuritySubmitDisabled()).toBe(true);
      });

      await test.step("the mismatch note follows focus-aware timing", async () => {
        await driver.settingsProfileSecurityFill("new", strongA);
        expect(await driver.settingsProfileSecurityStrengthVisible()).toBe(false);
        await driver.settingsProfileSecurityFill("confirm", "x");
        // Still focused and shorter than the guess: no note yet.
        expect(await driver.settingsProfileSecurityMismatchVisible()).toBe(false);
        await driver.settingsProfileSecurityBlur("confirm");
        expect(await driver.settingsProfileSecurityMismatchVisible()).toBe(true);
        // Longer than the guess: the note shows even while focused.
        await driver.settingsProfileSecurityFill("confirm", `${strongA}-zzz`);
        expect(await driver.settingsProfileSecurityMismatchVisible()).toBe(true);
        await driver.settingsProfileSecurityFill("confirm", strongA);
        expect(await driver.settingsProfileSecurityMismatchVisible()).toBe(false);
      });

      await test.step("a guess matching the old password is called out and blocks submit", async () => {
        await driver.settingsProfileSecurityFill("current", current);
        await driver.settingsProfileSecurityFill("new", current);
        await driver.settingsProfileSecurityBlur("new");
        expect(await driver.settingsProfileSecuritySameAsOldVisible()).toBe(true);
        expect(await driver.settingsProfileSecuritySubmitDisabled()).toBe(true);
        await driver.settingsProfileSecurityFill("new", strongB);
        await driver.settingsProfileSecurityFill("confirm", strongB);
        expect(await driver.settingsProfileSecuritySameAsOldVisible()).toBe(false);
        expect(await driver.settingsProfileSecuritySubmitDisabled()).toBe(false);
      });

      await test.step("a guess the server rates weak maps to a field error", async () => {
        // Client-strong (upper, lower, digit, special, long) but a common
        // pattern the server's estimator refuses.
        await driver.settingsProfileSecurityFill("new", "Password123!");
        await driver.settingsProfileSecurityFill("confirm", "Password123!");
        await driver.settingsProfileSecuritySubmit();
        await expect
          .poll(() => driver.settingsProfileSecurityFieldError("new"), { timeout: 30_000 })
          .toBe("Please use a stronger password.");
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Password too weak",
          });
      });

      await test.step("a wrong current password toasts without resetting or saving", async () => {
        await driver.settingsProfileSecurityFill("current", "Wrong-Pass-1!");
        await driver.settingsProfileSecurityFill("new", strongB);
        await driver.settingsProfileSecurityFill("confirm", strongB);
        await driver.settingsProfileSecuritySubmit();
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Incorrect old password",
            message: "Incorrect old password. Please try again.",
          });
        expect(await driver.settingsProfileSecurityFieldValue("new")).toBe(strongB);
        await expect(signInSession(member.email, current)).resolves.toContain("session-id=");
      });

      await test.step("the right current password saves, resets and takes effect", async () => {
        await driver.settingsProfileSecurityFill("current", current);
        await driver.settingsProfileSecuritySubmit();
        await expect
          .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
          .toMatchObject({
            title: "Success!",
            message: "Password changed successfully.",
          });
        for (const field of ["current", "new", "confirm"] as const) {
          expect(await driver.settingsProfileSecurityFieldValue(field)).toBe("");
        }
        await expect(signInSession(member.email, strongB)).resolves.toContain("session-id=");
        await expect(signInSession(member.email, current)).rejects.toThrow();
      });
    } finally {
      await cleanupMember(seed, ownerSession, member.userId);
    }
  }
);

test(
  specTitle(["PROF-015"], "provider-created accounts set a password with no current field"),
  { tag: specTags(["PROF-015"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    void seed;
    // The magic-code path requires instance mail even with a planted code.
    await setSmtpConfigured(true);
    let session: AuthedSession | undefined;
    try {
      const email = uniqueEmail("parity-pwless");
      const planted = await mintMagicCode(email);
      session = await serverMagicSignUp(email, planted);
    } finally {
      await setSmtpConfigured(false);
    }
    if (!session) throw new Error("[parity] magic sign-up produced no session.");
    const email = session.email;
    expect((await userFacts(email)).passwordAutoset).toBe(true);
    await serverCompleteOnboarding(session.cookie);

    await driver.settingsProfileOpen("security", browserSessionCookies(session));
    await awaitSecurity(driver);

    await test.step("the current-password field is skipped", async () => {
      expect(await driver.settingsProfileSecurityFieldVisible("current")).toBe(false);
      expect(await driver.settingsProfileSecurityFieldVisible("new")).toBe(true);
      expect(await driver.settingsProfileSecurityFieldVisible("confirm")).toBe(true);
    });

    await test.step("a fresh guess saves without the current password", async () => {
      const fresh = "Parity-First-7K";
      await driver.settingsProfileSecurityFill("new", fresh);
      await driver.settingsProfileSecurityFill("confirm", fresh);
      expect(await driver.settingsProfileSecuritySubmitDisabled()).toBe(false);
      await driver.settingsProfileSecuritySubmit();
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 30_000 })
        .toMatchObject({
          title: "Success!",
          message: "Password changed successfully.",
        });
      await expect(signInSession(email, fresh)).resolves.toContain("session-id=");
      expect((await userFacts(email)).passwordAutoset).toBe(false);
    });
  }
);
