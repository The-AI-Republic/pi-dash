// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-179): the add-runner modal — entries in both
// scopes with prefill+lock from project routes (RUN-006), cloud-driven
// creation on a connected machine (RUN-007), the generated CLI command with
// shell selector and copy (RUN-008), and the agent/model catalog (RUN-009).
// Rows: RUN-006, RUN-007, RUN-008, RUN-009.
//
// Fixtures: machines and control sessions are planted through the Django
// shell (the web API offers no create endpoints); pods and projects go
// through the app's own REST endpoints. The scratch stack runs no live
// daemon, so daemon write-backs are simulated through the command-result
// store exactly as the daemon endpoint would store them. The main#539
// supported-agents gating is absent from this checkout, so the gating
// halves of RUN-007/RUN-009 are not covered here.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  signInSession,
  projectFacts,
  projectPods,
  createPod,
  deletePod,
  createProject,
  serverWorkspaceIdBySlug,
  serverDevMachinesPlantMachine,
  serverDevMachinesCleanupMachine,
  serverDevMachinesListMachines,
  serverAddRunnerPlantSession,
  serverAddRunnerTouchSession,
  serverAddRunnerAgeSession,
  serverAddRunnerDropSession,
  serverAddRunnerSetResult,
  serverAddRunnerCreate,
  serverAddRunnerStatus,
  serverAddRunnerRunnerNames,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

let serial = 0;

/** Unique human label (machines, projects, pods shown by label). */
function tag(prefix: string): string {
  serial += 1;
  return `ar179 ${prefix} ${Date.now()} ${serial}`;
}

/** Unique runner/pod slug (runner names allow no spaces). */
function slug(prefix: string): string {
  serial += 1;
  return `${prefix}${Date.now().toString(36)}${serial}`;
}

/** Machine picker label when no connected machine is selected. */
const MANUAL_MACHINE = "Run `pidash runner add` manually";

async function openSignedIn(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
}

async function closeModal(driver: ParityDriver): Promise<void> {
  await driver.addRunnerClose();
  await expect.poll(() => driver.addRunnerVisible(), { timeout: 10_000 }).toBe(false);
}

test(
  specTitle(["RUN-006"], "modal opens from both pages and locks the project on project routes"),
  { tag: specTags(["RUN-006"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    expect(identifier).not.toBe("");
    await openSignedIn(driver, seed);

    await test.step("workspace runners entry opens the form unlocked", async () => {
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
      expect(await driver.addRunnerVisible()).toBe(true);
      expect(await driver.addRunnerLayout()).toBe("form");
      const form = await driver.addRunnerForm();
      expect(form.projectLocked).toBe(false);
      expect(form.project).toBe("Select a project");
      expect(form.pod).toBe("(default pod)");
      expect(form.agent).toBe("Claude Code");
      expect(form.model).toBe("Opus 4.8");
      expect(await driver.addRunnerProjectOptions()).toContain(seed.projectName);
      await closeModal(driver);
    });

    await test.step("machines-page entry opens the same form", async () => {
      await driver.addRunnerOpenFromMachines(seed.workspaceSlug);
      expect(await driver.addRunnerLayout()).toBe("form");
      const form = await driver.addRunnerForm();
      expect(form.projectLocked).toBe(false);
      expect(form.project).toBe("Select a project");
      await closeModal(driver);
    });

    await test.step("project route prefills and locks the project", async () => {
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug, seed.projectId);
      const form = await driver.addRunnerForm();
      expect(form.projectLocked).toBe(true);
      // The prefill lands once the projects list arrives.
      await expect.poll(async () => (await driver.addRunnerForm()).project, { timeout: 15_000 }).toBe(seed.projectName);
      // Server: the seeded project carries pods the picker can offer.
      const pods = await projectPods(seed.projectId, session);
      expect(pods.length).toBeGreaterThan(0);
      await closeModal(driver);
    });

    // Server: opening the modal wrote nothing.
    expect(await serverAddRunnerRunnerNames(wsId, session)).toEqual([]);
  }
);

test(
  specTitle(["RUN-006"], "project is required and the name is format-validated"),
  { tag: specTags(["RUN-006"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const before = await serverAddRunnerRunnerNames(wsId, session);
    await openSignedIn(driver, seed);
    await driver.addRunnerOpenFromRunners(seed.workspaceSlug);

    await test.step("empty submit demands a project and stays on the form", async () => {
      await driver.addRunnerSubmit();
      expect(await driver.addRunnerProjectError()).toBe("Pick a project.");
      expect(await driver.addRunnerLayout()).toBe("form");
    });

    await test.step("names with spaces or a bad start are rejected", async () => {
      await driver.addRunnerSetName("has space");
      await driver.addRunnerSubmit();
      expect(await driver.addRunnerNameError()).toContain("Runner name cannot contain spaces.");
      await driver.addRunnerSetName("-badstart");
      await driver.addRunnerSubmit();
      expect(await driver.addRunnerNameError()).toContain("Runner name cannot contain spaces.");
    });

    await test.step("a valid name clears the error; picking a project submits", async () => {
      await driver.addRunnerSetName("good_name-1.2");
      await driver.addRunnerSubmit();
      expect(await driver.addRunnerNameError()).toBeNull();
      expect(await driver.addRunnerProjectError()).toBe("Pick a project.");
      await driver.addRunnerPickProject(seed.projectName);
      // Manual path regardless of any machine another spec leaked.
      await driver.addRunnerPickManual();
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
      await closeModal(driver);
    });

    // Server: validation failures and the manual command wrote nothing.
    expect(await serverAddRunnerRunnerNames(wsId, session)).toEqual(before);
  }
);

test(
  specTitle(["RUN-006"], "project and pod cascade and the form resets on reopen"),
  { tag: specTags(["RUN-006"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const otherName = tag("cascade project");
    const otherIdentifier = `ARC${String(Date.now()).slice(-6)}`;
    const other = await createProject(seed.workspaceSlug, session, otherName, otherIdentifier);
    const podA = await createPod(seed.projectId, session, slug("arpod"));
    const podB = await createPod(other.id, session, slug("arpod"));
    try {
      await openSignedIn(driver, seed);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);

      await test.step("pod-first selection backfills the project", async () => {
        await driver.addRunnerPickPod(podA.name);
        await expect
          .poll(async () => (await driver.addRunnerForm()).project, { timeout: 15_000 })
          .toBe(seed.projectName);
      });

      await test.step("switching project clears the ill-fitting pod and narrows the list", async () => {
        await driver.addRunnerPickProject(otherName);
        await expect.poll(async () => (await driver.addRunnerForm()).pod, { timeout: 15_000 }).toBe("(default pod)");
        const narrowed = await driver.addRunnerPodOptions();
        expect(narrowed).toContain("(default pod)");
        expect(narrowed.some((option) => option.startsWith(podB.name))).toBe(true);
        expect(narrowed.some((option) => option.startsWith(podA.name))).toBe(false);
      });

      await test.step("reopen resets every field; no project shows every pod", async () => {
        await closeModal(driver);
        await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
        const fresh = await driver.addRunnerForm();
        expect(fresh.project).toBe("Select a project");
        expect(fresh.pod).toBe("(default pod)");
        expect(fresh.name).toBe("");
        const all = await driver.addRunnerPodOptions();
        expect(all.some((option) => option.startsWith(podA.name))).toBe(true);
        expect(all.some((option) => option.startsWith(podB.name))).toBe(true);
      });

      await test.step("entered values do not survive a reopen", async () => {
        await driver.addRunnerPickProject(seed.projectName);
        await driver.addRunnerSetName("reopen-reset-me");
        await closeModal(driver);
        await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
        const reset = await driver.addRunnerForm();
        expect(reset.project).toBe("Select a project");
        expect(reset.name).toBe("");
        expect(reset.model).toBe("Opus 4.8");
        await closeModal(driver);
      });
    } finally {
      await deletePod(podA.id, session);
      await deletePod(podB.id, session);
    }

    // Server: the cascade fixtures read back (minus the cleanup).
    const pods = await projectPods(seed.projectId, session);
    expect(pods.some((pod) => pod.name === podA.name)).toBe(false);
  }
);

/** Request id the modal is polling, read off the spied status URLs. */
function polledRequestId(urls: string[]): string {
  const first = urls[0] ?? "";
  const requestId = /\/create-runner\/([^/]+)\//.exec(first)?.[1] ?? "";
  if (requestId === "") throw new Error("[parity] no polled create-runner request observed.");
  return requestId;
}

test(
  specTitle(["RUN-007"], "creates the runner on a connected machine"),
  { tag: specTags(["RUN-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const label = tag("ok machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    await serverAddRunnerPlantSession(machine.id);
    try {
      // Server: the planted machine lists in the workspace.
      const rows = await serverDevMachinesListMachines(wsId, session);
      expect(rows.some((row) => row.id === machine.id)).toBe(true);

      await openSignedIn(driver, seed);
      await driver.addRunnerCreateSpyStart();
      await driver.addRunnerStatusSpyStart();
      // The sign-in may have eaten the 90s online window; refresh first.
      await serverAddRunnerTouchSession(machine.id);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);

      // The picker auto-selects a connected machine; then pin this one.
      await expect
        .poll(async () => (await driver.addRunnerForm()).machine, { timeout: 30_000 })
        .not.toBe(MANUAL_MACHINE);
      expect(await driver.addRunnerMachineOptions()).toContain(label);
      await driver.addRunnerPickMachine(label);
      await driver.addRunnerPickProject(seed.projectName);
      const runnerName = slug("arok");
      await driver.addRunnerSetName(runnerName);
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("creating");

      // The daemon reports back; the panel flips to success.
      await expect
        .poll(async () => (await driver.addRunnerStatusSpyUrls()).length, { timeout: 30_000 })
        .toBeGreaterThan(0);
      const requestId = polledRequestId(await driver.addRunnerStatusSpyUrls());
      await serverAddRunnerSetResult(requestId, {
        status: "ok",
        dev_machine_id: machine.id,
        runner_id: "",
        runner_name: runnerName,
      });
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("ok");
      expect(await driver.addRunnerRemoteRunnerName()).toBe(runnerName);

      // The create POST carried the entered values.
      const bodies = await driver.addRunnerCreateSpyBodies();
      expect(bodies.length).toBe(1);
      const posted = JSON.parse(bodies[0] ?? "{}") as Record<string, unknown>;
      expect(posted["project"]).toBe(identifier);
      expect(posted["name"]).toBe(runnerName);
      expect(posted["agent"]).toBe("claude-code");
      expect(posted["model"]).toBe("claude-opus-4-8");
      expect(posted["reasoning_effort"]).toBeUndefined();

      // Server: the status endpoint reports the daemon's verdict.
      const status = await serverAddRunnerStatus(machine.id, requestId, wsId, session);
      expect(status.status).toBe(200);
      expect(status.body["status"]).toBe("ok");
      expect(status.body["runner_name"]).toBe(runnerName);

      await closeModal(driver);
    } finally {
      await driver.addRunnerCreateSpyStop();
      await driver.addRunnerStatusSpyStop();
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-007"], "surfaces daemon errors and carries values to the manual command"),
  { tag: specTags(["RUN-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const label = tag("error machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    await serverAddRunnerPlantSession(machine.id);
    const pod = await createPod(seed.projectId, session, slug("arpod"));
    try {
      await openSignedIn(driver, seed);
      await driver.addRunnerCreateSpyStart();
      await driver.addRunnerStatusSpyStart();
      // The sign-in may have eaten the 90s online window; refresh first.
      await serverAddRunnerTouchSession(machine.id);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.addRunnerForm()).machine, { timeout: 30_000 })
        .not.toBe(MANUAL_MACHINE);
      await driver.addRunnerPickMachine(label);
      await driver.addRunnerPickProject(seed.projectName);
      await driver.addRunnerPickPod(pod.name);
      const runnerName = slug("arerr");
      await driver.addRunnerSetName(runnerName);
      await driver.addRunnerSetWorkingDir("/tmp/parity work");
      await driver.addRunnerPickAgent("Codex");
      await driver.addRunnerPickModel("GPT-5.6 Sol (High)");
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("creating");

      await expect
        .poll(async () => (await driver.addRunnerStatusSpyUrls()).length, { timeout: 30_000 })
        .toBeGreaterThan(0);
      const requestId = polledRequestId(await driver.addRunnerStatusSpyUrls());
      await serverAddRunnerSetResult(requestId, {
        status: "error",
        dev_machine_id: machine.id,
        error: "parity boom",
      });
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("error");
      expect(await driver.addRunnerRemoteText()).toContain("parity boom");

      // The create POST carried the full form (pod, working dir, effort).
      const posted = JSON.parse((await driver.addRunnerCreateSpyBodies())[0] ?? "{}") as Record<string, unknown>;
      expect(posted["pod"]).toBe(pod.name);
      expect(posted["working_dir"]).toBe("/tmp/parity work");
      expect(posted["agent"]).toBe("codex");
      expect(posted["model"]).toBe("gpt-5.6-sol");
      expect(posted["reasoning_effort"]).toBe("high");

      // Server: the status endpoint reports the daemon's error.
      const status = await serverAddRunnerStatus(machine.id, requestId, wsId, session);
      expect(status.body["status"]).toBe("error");

      // Carry-over: the manual command keeps every entered value.
      await driver.addRunnerRemoteManual();
      await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
      const command = (await driver.addRunnerCommandText()) ?? "";
      expect(command).toContain(`--project ${identifier}`);
      expect(command).toContain(`--pod ${pod.name}`);
      expect(command).toContain(`--name ${runnerName}`);
      expect(command).toContain("--working-dir '/tmp/parity work'");
      expect(command).toContain("--agent codex");
      expect(command).toContain("--model gpt-5.6-sol");
      expect(command).toContain("--reasoning-effort high");
      await closeModal(driver);
    } finally {
      await driver.addRunnerCreateSpyStop();
      await driver.addRunnerStatusSpyStop();
      await deletePod(pod.id, session);
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-007"], "times out when the daemon stays silent and offers the manual command"),
  { tag: specTags(["RUN-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const label = tag("timeout machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    await serverAddRunnerPlantSession(machine.id);
    try {
      await openSignedIn(driver, seed);
      await driver.addRunnerStatusSpyStart();
      // The sign-in may have eaten the 90s online window; refresh first.
      await serverAddRunnerTouchSession(machine.id);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
      await expect
        .poll(async () => (await driver.addRunnerForm()).machine, { timeout: 30_000 })
        .not.toBe(MANUAL_MACHINE);
      await driver.addRunnerPickMachine(label);
      await driver.addRunnerPickProject(seed.projectName);
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("creating");

      // No daemon write-back: the 90s poll budget expires into the timeout panel.
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 150_000 }).toBe("timeout");
      expect(await driver.addRunnerRemoteText()).toContain("did not report back");

      // Server: the request is still pending (the daemon may yet report).
      const requestId = polledRequestId(await driver.addRunnerStatusSpyUrls());
      const status = await serverAddRunnerStatus(machine.id, requestId, wsId, session);
      expect(status.body["status"]).toBe("pending");

      await driver.addRunnerRemoteManual();
      await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
      expect((await driver.addRunnerCommandText()) ?? "").toContain(`--project ${identifier}`);
      await closeModal(driver);
    } finally {
      await driver.addRunnerStatusSpyStop();
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-007"], "snaps back when the machine drops and cancels the poll on close"),
  { tag: specTags(["RUN-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const labelA = tag("snap machine");
    const machineA = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: labelA,
    });
    await serverAddRunnerPlantSession(machineA.id);
    const labelB = tag("cancel machine");
    const machineB = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: labelB,
    });
    await serverAddRunnerPlantSession(machineB.id);
    try {
      // Server: both planted machines list in the workspace.
      const rows = await serverDevMachinesListMachines(wsId, session);
      expect(rows.some((row) => row.id === machineA.id)).toBe(true);
      expect(rows.some((row) => row.id === machineB.id)).toBe(true);

      await openSignedIn(driver, seed);
      // The sign-in may have eaten the 90s online window; refresh first.
      await serverAddRunnerTouchSession(machineA.id);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);

      await test.step("picker snaps back to manual once the machine drops", async () => {
        await driver.addRunnerPickMachine(labelA);
        expect(await (async () => (await driver.addRunnerForm()).machine)()).toBe(labelA);
        await serverAddRunnerAgeSession(machineA.id, 120);
        // The 15s list refresh observes the stale session and snaps back.
        await expect.poll(async () => (await driver.addRunnerForm()).machine, { timeout: 45_000 }).toBe(MANUAL_MACHINE);
      });

      await test.step("closing the modal stops the status poll", async () => {
        await closeModal(driver);
        await driver.addRunnerStatusSpyStart();
        // Part 1 aged this session past the online window; refresh first.
        await serverAddRunnerTouchSession(machineB.id);
        await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
        await driver.addRunnerPickMachine(labelB);
        await driver.addRunnerPickProject(seed.projectName);
        await driver.addRunnerSubmit();
        await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("creating");
        await expect
          .poll(async () => (await driver.addRunnerStatusSpyUrls()).length, { timeout: 30_000 })
          .toBeGreaterThan(0);
        await closeModal(driver);
        const settled = (await driver.addRunnerStatusSpyUrls()).length;
        // The fetch already in flight when Close lands may still complete
        // (at most one trailing poll); a surviving loop would fire three
        // more polls in 7s.
        await driver.page.waitForTimeout(7_000);
        expect((await driver.addRunnerStatusSpyUrls()).length).toBeLessThanOrEqual(settled + 1);
      });
    } finally {
      await driver.addRunnerStatusSpyStop();
      await serverDevMachinesCleanupMachine(machineA.id);
      await serverDevMachinesCleanupMachine(machineB.id);
    }
  }
);

test(
  specTitle(["RUN-007"], "reports when the machine drops between picker load and submit"),
  { tag: specTags(["RUN-007"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const label = tag("offline machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    await serverAddRunnerPlantSession(machine.id);
    try {
      // Server: the planted machine lists in the workspace.
      const rows = await serverDevMachinesListMachines(wsId, session);
      expect(rows.some((row) => row.id === machine.id)).toBe(true);

      await openSignedIn(driver, seed);
      await driver.addRunnerCreateSpyStart();
      // The sign-in may have eaten the 90s online window; refresh first.
      await serverAddRunnerTouchSession(machine.id);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
      await driver.addRunnerPickMachine(label);
      await driver.addRunnerPickProject(seed.projectName);
      // Drop the control session after the picker loaded: delivery fails.
      await serverAddRunnerDropSession(machine.id);
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerRemotePhase(), { timeout: 30_000 }).toBe("error");
      expect(await driver.addRunnerRemoteText()).toContain("went offline");
      // The POST was attempted (the 409 came from the server, not the client).
      expect((await driver.addRunnerCreateSpyBodies()).length).toBe(1);
      await closeModal(driver);
    } finally {
      await driver.addRunnerCreateSpyStop();
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-008"], "builds the quoted command per shell with copy and origin fallback"),
  { tag: specTags(["RUN-008"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const before = await serverAddRunnerRunnerNames(wsId, session);
    const pod = await createPod(seed.projectId, session, slug("arpod"));
    try {
      await openSignedIn(driver, seed);
      await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
      await driver.addRunnerPickManual();
      await driver.addRunnerPickProject(seed.projectName);
      await driver.addRunnerPickPod(pod.name);
      const runnerName = slug("arcmd");
      await driver.addRunnerSetName(runnerName);
      await driver.addRunnerSetWorkingDir("/tmp/parity work");
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
      expect(await driver.addRunnerCommandHeader()).toContain(identifier);

      await test.step("posix renders multi-line with single-quote escaping", async () => {
        expect(await driver.addRunnerShellOptions()).toEqual(["macOS/Linux", "PowerShell", "Command Prompt"]);
        expect(await driver.addRunnerActiveShell()).toBe("macOS/Linux");
        const posix = (await driver.addRunnerCommandText()) ?? "";
        expect(posix).toContain("pidash runner add \\");
        expect(posix.split("\n").length).toBeGreaterThan(1);
        expect(posix).toContain(`--project ${identifier}`);
        expect(posix).toContain(`--pod ${pod.name}`);
        expect(posix).toContain(`--name ${runnerName}`);
        expect(posix).toContain("--working-dir '/tmp/parity work'");
        expect(posix).toContain("--agent claude-code");
        expect(posix).toContain("--model claude-opus-4-8");
        expect(posix).not.toContain("--reasoning-effort");
      });

      await test.step("powershell and cmd render single-line with their own quoting", async () => {
        await driver.addRunnerPickShell("PowerShell");
        expect(await driver.addRunnerActiveShell()).toBe("PowerShell");
        const ps = (await driver.addRunnerCommandText()) ?? "";
        expect(ps).not.toContain("\n");
        expect(ps).toContain("pidash runner add --url ");
        expect(ps).toContain("--working-dir '/tmp/parity work'");
        await driver.addRunnerPickShell("Command Prompt");
        expect(await driver.addRunnerActiveShell()).toBe("Command Prompt");
        const cmd = (await driver.addRunnerCommandText()) ?? "";
        expect(cmd).not.toContain("\n");
        expect(cmd).toContain('--working-dir "/tmp/parity work"');
      });

      await test.step("copy writes the command with transient confirmation", async () => {
        const cmd = (await driver.addRunnerCommandText()) ?? "";
        await driver.addRunnerCopy();
        await expect.poll(() => driver.addRunnerCopyState(), { timeout: 10_000 }).toBe("Copied!");
        expect(await driver.addRunnerReadClipboard()).toBe(cmd);
        await expect.poll(() => driver.addRunnerCopyState(), { timeout: 10_000 }).toBe("Copy command");
      });

      await test.step("copy failure toasts an error", async () => {
        await driver.addRunnerBreakClipboard();
        await driver.addRunnerCopy();
        await expect
          .poll(() => driver.addRunnerLastToast(), { timeout: 10_000 })
          .toContain("Could not copy to clipboard");
      });

      await test.step("no API base URL falls back to the browser origin", async () => {
        // The oracle builds with an empty API base URL, so the command
        // targets the serving origin and says so.
        const oracleUrl = process.env["PARITY_ORACLE_URL"] ?? "http://localhost:13000";
        const cmd = (await driver.addRunnerCommandText()) ?? "";
        expect(cmd).toContain(`--url ${oracleUrl}`);
        expect(await driver.addRunnerOriginNote()).toContain("VITE_API_BASE_URL");
      });

      await closeModal(driver);
    } finally {
      await deletePod(pod.id, session);
    }

    // Server: the manual command wrote nothing.
    expect(await serverAddRunnerRunnerNames(wsId, session)).toEqual(before);
  }
);

test(
  specTitle(["RUN-009"], "catalogs agent models and resets the model on agent change"),
  { tag: specTags(["RUN-009"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    // Server: the seeded project (whose name the cascade proves) reads back.
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    expect(identifier).not.toBe("");
    await openSignedIn(driver, seed);
    await driver.addRunnerOpenFromRunners(seed.workspaceSlug);

    await test.step("six agents with per-agent model lists", async () => {
      expect(await driver.addRunnerAgentOptions()).toEqual([
        "Claude Code",
        "Codex",
        "Cursor",
        "OpenClaw",
        "Grok",
        "Muse Code",
      ]);
      // Claude Code pins Opus 4.8 rather than the agent default.
      expect((await driver.addRunnerForm()).model).toBe("Opus 4.8");
      const claude = await driver.addRunnerModelOptions();
      expect(claude.length).toBe(6);
      expect(claude).toContain("Default (agent's built-in model)");
      expect(claude).toContain("Fable 5");
      await driver.addRunnerPickAgent("Cursor");
      await expect
        .poll(async () => (await driver.addRunnerForm()).model, { timeout: 10_000 })
        .toBe("Default (agent's built-in model)");
      expect((await driver.addRunnerModelOptions()).length).toBe(13);
      await driver.addRunnerPickAgent("Codex");
      const codex = await driver.addRunnerModelOptions();
      expect(codex.length).toBe(29);
      expect(codex).toContain("GPT-5.6 Sol (High)");
      await driver.addRunnerPickAgent("OpenClaw");
      expect(await driver.addRunnerModelOptions()).toEqual(["Default (agent's built-in model)"]);
      await driver.addRunnerPickAgent("Grok");
      expect((await driver.addRunnerModelOptions()).length).toBe(3);
      await driver.addRunnerPickAgent("Muse Code");
      expect(await driver.addRunnerModelOptions()).toEqual(["Default (agent's built-in model)"]);
    });

    await test.step("agent change resets a stale model; codex carries reasoning effort", async () => {
      await driver.addRunnerPickAgent("Claude Code");
      await driver.addRunnerPickModel("Sonnet 4.6");
      await expect.poll(async () => (await driver.addRunnerForm()).model, { timeout: 10_000 }).toBe("Sonnet 4.6");
      await driver.addRunnerPickAgent("Codex");
      await expect
        .poll(async () => (await driver.addRunnerForm()).model, { timeout: 10_000 })
        .toBe("Default (agent's built-in model)");
      await driver.addRunnerPickModel("GPT-5.6 Sol (High)");
      await driver.addRunnerPickManual();
      await driver.addRunnerPickProject(seed.projectName);
      await driver.addRunnerSubmit();
      await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
      const command = (await driver.addRunnerCommandText()) ?? "";
      expect(command).toContain("--agent codex");
      expect(command).toContain("--model gpt-5.6-sol");
      expect(command).toContain("--reasoning-effort high");
      await closeModal(driver);
    });
  }
);

test(
  specTitle(["RUN-009"], "default sentinel omits flags and unknown models degrade"),
  { tag: specTags(["RUN-009"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const label = tag("catalog machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    await serverAddRunnerPlantSession(machine.id);
    try {
      await test.step("unknown model is accepted, not validated", async () => {
        // The catalog is advisory: the server takes any model slug.
        const created = await serverAddRunnerCreate(
          machine.id,
          wsId,
          { project: identifier, agent: "codex", model: "not-a-real-model-179" },
          session
        );
        expect(created.status).toBe(202);
        const requestId = (created.body as { request_id?: unknown }).request_id;
        expect(typeof requestId).toBe("string");
        const status = await serverAddRunnerStatus(machine.id, String(requestId), wsId, session);
        expect(status.body["status"]).toBe("pending");
      });

      await test.step("default sentinel omits the model flag; empty fields omit theirs", async () => {
        await openSignedIn(driver, seed);
        await driver.addRunnerOpenFromRunners(seed.workspaceSlug);
        await driver.addRunnerPickManual();
        await driver.addRunnerPickAgent("Codex");
        expect((await driver.addRunnerForm()).model).toBe("Default (agent's built-in model)");
        await driver.addRunnerPickProject(seed.projectName);
        await driver.addRunnerSubmit();
        await expect.poll(() => driver.addRunnerLayout(), { timeout: 15_000 }).toBe("command");
        const command = (await driver.addRunnerCommandText()) ?? "";
        expect(command).toContain("--agent codex");
        expect(command).not.toContain("--model");
        expect(command).not.toContain("--reasoning-effort");
        expect(command).not.toContain("--pod");
        expect(command).not.toContain("--name");
        expect(command).not.toContain("--working-dir");
        await closeModal(driver);
      });
    } finally {
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);
