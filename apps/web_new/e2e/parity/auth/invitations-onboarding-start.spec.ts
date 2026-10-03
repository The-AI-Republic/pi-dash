// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle parity scenarios (NEWFRONT-110): auth invitations plus onboarding
// start. Green on apps/web first (the oracle); the same file must go green
// on apps/web_new once the auth area lands. Rows: AUTH-026 (invitation
// inbox plus batch accept), AUTH-027 (single-invitation accept/decline
// including stale states), AUTH-028 (post-invitation landing), AUTH-029
// (onboarding gate plus resume), AUTH-030 (CLI-install step), AUTH-031
// (profile-setup step), AUTH-032 (role-selection step), AUTH-033 (use-case
// step). The seed user arrives onboarded, so onboarding scenarios mint a
// fresh account per test over the native sign-up endpoint, and invitation
// scenarios create real invites over the API (unique invitee addresses;
// answered variants for the stale states). The invite model carries no
// expiry field, so the stale variants are answered-accepted,
// answered-declined, and unresolvable links. The parity stack reports
// is_self_managed, so AUTH-032/AUTH-033 prove the designed skip of the
// role and use-case steps; their choose-and-require UI only exists on
// non-self-managed instances and stays covered by the driver methods.
import { test, expect } from "../fixtures";
import {
  answerSingleInvitation,
  createAccountSession,
  createWorkspace,
  createWorkspaceInvites,
  currentUser,
  markSessionOnboarded,
  myInvitations,
  signInSession,
  uniqueEmail,
  userProfile,
  workspaceMemberEmails,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const FRESH_PASSWORD = "Parity-Fresh-1!";

/** Mint a fresh account and return its address plus API session. */
async function freshAccount(prefix: string): Promise<{ email: string; session: string }> {
  const email = uniqueEmail(prefix);
  const session = await createAccountSession(email, FRESH_PASSWORD);
  return { email, session };
}

/** Mint a fresh account, mark it onboarded, and sign it in through the UI. */
async function onboardedInvitee(
  driver: {
    openEntry(): Promise<void>;
    signInWithPassword(email: string, password: string): Promise<void>;
  },
  prefix: string
): Promise<{ email: string; session: string }> {
  const account = await freshAccount(prefix);
  await markSessionOnboarded(account.session);
  await driver.openEntry();
  await driver.signInWithPassword(account.email, FRESH_PASSWORD);
  return account;
}

test(
  specTitle(["AUTH-026"], "invitation inbox lists invites and batch-accepts them at once"),
  { tag: specTags(["AUTH-026"]) },
  async ({ driver, seed }) => {
    const adminSession = await signInSession(seed.email, seed.password);
    const secondSlug = `parity-extra-${Date.now().toString(36)}`;
    let invitee: { email: string; session: string };
    await test.step("invite one address to two workspaces", async () => {
      await createWorkspace(adminSession, "Parity Extra", secondSlug);
      invitee = await freshAccount("parish-inbox");
      await markSessionOnboarded(invitee.session);
      await createWorkspaceInvites(seed.workspaceSlug, adminSession, [{ email: invitee.email, role: 15 }]);
      await createWorkspaceInvites(secondSlug, adminSession, [{ email: invitee.email, role: 15 }]);
    });

    await test.step("sign in and open the inbox", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(invitee!.email, FRESH_PASSWORD);
      await driver.openInvitations();
    });

    await test.step("both invites are listed as cards", async () => {
      await expect
        .poll(() => driver.invitationWorkspaceNames(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining([seed.workspaceName, "Parity Extra"]));
    });

    await test.step("select both and accept at once", async () => {
      await driver.toggleInvitation(seed.workspaceName);
      await driver.toggleInvitation("Parity Extra");
      await driver.acceptSelectedInvitations();
    });

    await test.step("landing is inside the first joined workspace", async () => {
      await expect.poll(() => driver.page.url(), { timeout: 60_000 }).toContain(`/${seed.workspaceSlug}`);
    });

    await test.step("the server joined both workspaces", async () => {
      const first = await workspaceMemberEmails(seed.workspaceSlug, adminSession);
      const second = await workspaceMemberEmails(secondSlug, adminSession);
      expect(first).toContain(invitee!.email);
      expect(second).toContain(invitee!.email);
      expect(await myInvitations(invitee!.session)).toEqual([]);
    });
  }
);

test(
  specTitle(["AUTH-026"], "empty inbox shows the no-invites state with a way home"),
  { tag: specTags(["AUTH-026"]) },
  async ({ driver, seed }) => {
    const invitee = await freshAccount("parish-empty");
    await markSessionOnboarded(invitee.session);

    await driver.openEntry();
    await driver.signInWithPassword(invitee.email, FRESH_PASSWORD);
    await driver.openInvitations();

    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("No pending invites");
    expect(await driver.invitationWorkspaceNames()).toEqual([]);
    void seed;
  }
);

test(
  specTitle(["AUTH-027"], "single-invitation link accepts a matching signed-in recipient"),
  { tag: specTags(["AUTH-027"]) },
  async ({ driver, seed }) => {
    const adminSession = await signInSession(seed.email, seed.password);
    const invitee = await freshAccount("parish-single");
    await markSessionOnboarded(invitee.session);
    const [invite] = await createWorkspaceInvites(seed.workspaceSlug, adminSession, [
      { email: invitee.email, role: 15 },
    ]);

    await driver.openEntry();
    await driver.signInWithPassword(invitee.email, FRESH_PASSWORD);
    await driver.openInvitationLink(seed.workspaceSlug, invite!.id, invite!.token);

    await test.step("the pending card names the workspace", async () => {
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain(seed.workspaceName);
    });

    await driver.acceptSingleInvitation();

    await test.step("landing is inside the joined workspace", async () => {
      await expect.poll(() => driver.page.url(), { timeout: 60_000 }).toContain(`/${seed.workspaceSlug}`);
    });

    await test.step("the server recorded the membership", async () => {
      expect(await workspaceMemberEmails(seed.workspaceSlug, adminSession)).toContain(invitee.email);
    });
  }
);

test(
  specTitle(["AUTH-027", "AUTH-028"], "single-invitation link declines and always lands home"),
  { tag: specTags(["AUTH-027", "AUTH-028"]) },
  async ({ driver, seed }) => {
    const adminSession = await signInSession(seed.email, seed.password);
    const invitee = await freshAccount("parish-decline");
    await markSessionOnboarded(invitee.session);
    const [invite] = await createWorkspaceInvites(seed.workspaceSlug, adminSession, [
      { email: invitee.email, role: 15 },
    ]);

    await driver.openEntry();
    await driver.signInWithPassword(invitee.email, FRESH_PASSWORD);
    await driver.openInvitationLink(seed.workspaceSlug, invite!.id, invite!.token);
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain(seed.workspaceName);

    await driver.declineSingleInvitation();

    await test.step("ignoring lands home", async () => {
      await expect
        .poll(
          async () => {
            const url = new URL(driver.page.url());
            // Home is the bare origin (optional trailing slash); anything
            // deeper means we landed somewhere else.
            return url.pathname === "/" ? "home" : `elsewhere:${url.pathname}`;
          },
          { timeout: 60_000 }
        )
        .toBe("home");
    });

    await test.step("the server did not add a membership", async () => {
      expect(await workspaceMemberEmails(seed.workspaceSlug, adminSession)).not.toContain(invitee.email);
    });
  }
);

test(
  specTitle(["AUTH-027"], "already-answered accepted link renders the already-member state"),
  { tag: specTags(["AUTH-027"]) },
  async ({ driver, seed }) => {
    // Accepting an invite whose address has no account yet leaves the
    // answered-accepted row in place (the backend only deletes the row
    // when it can create a membership), so revisiting the link renders
    // the already-member card instead of the actions.
    const adminSession = await signInSession(seed.email, seed.password);
    const stranger = uniqueEmail("parish-answered");
    const [invite] = await createWorkspaceInvites(seed.workspaceSlug, adminSession, [{ email: stranger, role: 15 }]);
    await answerSingleInvitation(seed.workspaceSlug, invite!.id, true, invite!.token, adminSession);

    await driver.openInvitationLink(seed.workspaceSlug, invite!.id, invite!.token);

    await test.step("the actions are replaced by the already-member state", async () => {
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("already a member");
    });
  }
);

test(
  specTitle(["AUTH-027"], "already-answered declined link renders the inactive state"),
  { tag: specTags(["AUTH-027"]) },
  async ({ driver, seed }) => {
    const adminSession = await signInSession(seed.email, seed.password);
    const invitee = await freshAccount("parish-inactive");
    await markSessionOnboarded(invitee.session);
    const [invite] = await createWorkspaceInvites(seed.workspaceSlug, adminSession, [
      { email: invitee.email, role: 15 },
    ]);
    await answerSingleInvitation(seed.workspaceSlug, invite!.id, false, invite!.token, invitee.session);

    await driver.openEntry();
    await driver.signInWithPassword(invitee.email, FRESH_PASSWORD);
    await driver.openInvitationLink(seed.workspaceSlug, invite!.id, invite!.token);

    await test.step("the actions are replaced by the inactive-link state", async () => {
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("not active anymore");
    });
  }
);

test(
  specTitle(["AUTH-027"], "broken invitation link renders the inactive state"),
  { tag: specTags(["AUTH-027"]) },
  async ({ driver, seed }) => {
    // A link no invitation answers renders the inactive card, not the
    // "INVITATION NOT FOUND" card: that branch needs fetched detail plus a
    // fetch error at once, which a fresh broken link never produces.
    const invitee = await onboardedInvitee(driver, "parish-broken");

    await driver.openInvitationLink(seed.workspaceSlug, "00000000-0000-4000-8000-000000000000", "bogus");
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("not active anymore");
    void invitee;
  }
);

test(
  specTitle(["AUTH-028"], "mismatched signed-in address lands home after answering"),
  { tag: specTags(["AUTH-028"]) },
  async ({ driver, seed }) => {
    const adminSession = await signInSession(seed.email, seed.password);
    const stranger = uniqueEmail("parish-stranger");
    const [invite] = await createWorkspaceInvites(seed.workspaceSlug, adminSession, [{ email: stranger, role: 15 }]);

    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openInvitationLink(seed.workspaceSlug, invite!.id, invite!.token);
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain(seed.workspaceName);

    await driver.acceptSingleInvitation();

    await expect
      .poll(
        async () => {
          const url = new URL(driver.page.url());
          return url.pathname === "/" ? "home" : `elsewhere:${url.pathname}`;
        },
        { timeout: 60_000 }
      )
      .toBe("home");
  }
);

test(
  specTitle(["AUTH-029"], "onboarding gate funnels unfinished users and bounces finished ones away"),
  { tag: specTags(["AUTH-029"]) },
  async ({ driver, seed }) => {
    await test.step("signed-out visitors bounce to sign-in", async () => {
      await driver.openOnboarding();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Welcome back to Pi Dash");
    });

    await test.step("a fresh account lands in onboarding", async () => {
      const newcomer = await freshAccount("parish-gate");
      await driver.openEntry();
      await driver.signInWithPassword(newcomer.email, FRESH_PASSWORD);
      await driver.openOnboarding();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");
    });

    await test.step("a finished account bounces away from onboarding", async () => {
      const done = await onboardedInvitee(driver, "parish-gate-done");
      await driver.openOnboarding();
      await expect
        .poll(
          async () => {
            const text = await driver.pageText();
            return text.includes("Install the Pi Dash CLI") ? "still-onboarding" : "bounced";
          },
          { timeout: 60_000 }
        )
        .toBe("bounced");
      void done;
      void seed;
    });
  }
);

test(
  specTitle(["AUTH-029"], "refresh resumes at the workspace step once the profile is stored"),
  { tag: specTags(["AUTH-029"]) },
  async ({ driver }) => {
    // This stack is self-managed, so the profile step jumps straight to
    // the workspace step; the resume assertion below proves the stored
    // progress flag drives a reload back to the same step.
    const newcomer = await freshAccount("parish-resume");
    await driver.openEntry();
    await driver.signInWithPassword(newcomer.email, FRESH_PASSWORD);
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");

    await driver.skipCliInstall();
    await driver.submitProfileStep("Parity Resume");
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your workspace");

    await test.step("a reload resumes at the workspace step", async () => {
      await driver.openOnboarding();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your workspace");
    });

    await test.step("the server stored the profile progress flag", async () => {
      const profile = await userProfile(newcomer.session);
      expect(profile.onboarding_step?.profile_complete).toBe(true);
    });
  }
);

test(
  specTitle(["AUTH-030"], "CLI-install step shows per-OS guidance and both continues advance"),
  { tag: specTags(["AUTH-030"]) },
  async ({ driver }) => {
    const first = await freshAccount("parish-cli");
    await driver.openEntry();
    await driver.signInWithPassword(first.email, FRESH_PASSWORD);
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");

    await test.step("the primary continue reaches the profile step", async () => {
      const text = await driver.pageText();
      expect(text).toContain("macOS / Linux");
      await driver.advanceCliInstall();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your profile");
    });

    await test.step("skip reaches the profile step too and stores nothing", async () => {
      const second = await freshAccount("parish-cli-skip");
      await driver.openEntry();
      await driver.signInWithPassword(second.email, FRESH_PASSWORD);
      await driver.openOnboarding();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");
      await driver.skipCliInstall();
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your profile");
      expect((await userProfile(second.session)).onboarding_step ?? {}).toEqual({});
    });
  }
);

test(
  specTitle(["AUTH-031"], "profile step requires a name, saves it, and advances"),
  { tag: specTags(["AUTH-031"]) },
  async ({ driver }) => {
    const newcomer = await freshAccount("parish-profile");
    await driver.openEntry();
    await driver.signInWithPassword(newcomer.email, FRESH_PASSWORD);
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");
    await driver.skipCliInstall();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your profile");

    await test.step("submit stays disabled with an empty name", async () => {
      const continueButton = driver.page.getByRole("button", { name: /^continue$/i });
      await expect(continueButton).toBeDisabled();
    });

    await driver.submitProfileStep("Parity Profiler");
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your workspace");

    await test.step("the server stored the name", async () => {
      const user = await currentUser(newcomer.session);
      expect(`${user.first_name ?? ""} ${user.last_name ?? ""}`.trim()).not.toBe("");
    });
  }
);

test(
  specTitle(["AUTH-032"], "self-managed instances skip the role step entirely"),
  { tag: specTags(["AUTH-032"]) },
  async ({ driver }) => {
    // The parity stack reports is_self_managed, so after the profile step
    // the flow jumps straight to the workspace step: no role question is
    // ever shown and nothing is stored. The choose-and-require UI only
    // exists on non-self-managed instances (cloud edition overlay, not
    // part of this stack), which the driver methods still cover.
    const newcomer = await freshAccount("parish-role");
    await driver.openEntry();
    await driver.signInWithPassword(newcomer.email, FRESH_PASSWORD);
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");
    await driver.skipCliInstall();
    await driver.submitProfileStep("Parity Roler");

    await test.step("the workspace step follows the profile step directly", async () => {
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your workspace");
      expect(await driver.pageText()).not.toContain("What's your role?");
    });

    await test.step("the server stored no role", async () => {
      expect((await userProfile(newcomer.session)).role ?? "").toBe("");
    });
  }
);

test(
  specTitle(["AUTH-033"], "self-managed instances skip the use-case step entirely"),
  { tag: specTags(["AUTH-033"]) },
  async ({ driver }) => {
    // Same self-managed skip as the role step: the use-case question never
    // appears and nothing is stored. The multi-select-and-require UI only
    // exists on non-self-managed instances (cloud edition overlay, not
    // part of this stack), which the driver methods still cover.
    const newcomer = await freshAccount("parish-use");
    await driver.openEntry();
    await driver.signInWithPassword(newcomer.email, FRESH_PASSWORD);
    await driver.openOnboarding();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Install the Pi Dash CLI");
    await driver.skipCliInstall();
    await driver.submitProfileStep("Parity User");

    await test.step("the workspace step follows the profile step directly", async () => {
      await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your workspace");
      expect(await driver.pageText()).not.toContain("What brings you to Pi Dash?");
    });

    await test.step("the server stored no use cases", async () => {
      expect((await userProfile(newcomer.session)).use_case ?? "").toBe("");
    });
  }
);
