// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-183): AI dev machines, runner detail, agent
// activity — the machines list with client-derived statuses, token
// rotation, machine revoke and delete, the pidash CLI install section,
// the single-runner detail page, and the agent activity/status panel.
// Rows: RUN-037, RUN-038, RUN-039, RUN-040, RUN-041, RUN-042, RUN-043.
//
// Fixtures: machines, tokens, runners and live states are planted through
// the Django shell (the web API offers no create endpoints); every
// scenario also asserts the resulting server state through the app's own
// REST endpoints. Empty/loading/error surfaces use test-only network
// shaping; the scratch stack runs no live runner daemon.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  signInSession,
  serverDevMachinesWorkspaceId,
  serverDevMachinesPlantMachine,
  serverDevMachinesCleanupMachine,
  serverDevMachinesPlantRunner,
  serverDevMachinesCleanupRunner,
  serverDevMachinesPlantLiveState,
  serverDevMachinesClearLiveState,
  serverDevMachinesMachineExists,
  serverDevMachinesTokenRevocations,
  serverDevMachinesSessionRevocations,
  serverDevMachinesListMachines,
  serverDevMachinesRunnerDetail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

let serial = 0;

function tag(prefix: string): string {
  serial += 1;
  return `dm183 ${prefix} ${Date.now()} ${serial}`;
}

async function openSignedInMachines(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.devMachinesOpen(seed.workspaceSlug);
}

async function openSignedInDetail(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  runnerId: string,
  projectId?: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.runnerDetailOpen(seed.workspaceSlug, runnerId, projectId);
}

function metaValue(meta: { label: string; value: string }[], label: string): string | null {
  return meta.find((entry) => entry.label === label)?.value ?? null;
}

test(
  specTitle(["RUN-037"], "machines list shows planted machines with derived statuses"),
  { tag: specTags(["RUN-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDevMachinesWorkspaceId(seed.workspaceSlug);
    const activeLabel = tag("list-active");
    const offlineLabel = tag("list-offline");
    const offlineHost = tag("list-offline-host");
    const registeredLabel = tag("list-registered");
    const revokedLabel = tag("list-revoked");
    const active = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: activeLabel,
    });
    const offline = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: offlineLabel,
      hostLabel: offlineHost,
    });
    const registered = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: registeredLabel,
    });
    const revoked = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: revokedLabel,
      revoked: true,
    });
    const onlineRunner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: active.id,
      name: tag("list-active-runner"),
      status: "online",
      lastHeartbeatAgeSecs: 30,
    });
    const offlineRunner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: offline.id,
      name: tag("list-offline-runner"),
      status: "offline",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(activeLabel), { timeout: 15_000 }).not.toBeNull();

      await test.step("each row shows its derived status, counts and actions", async () => {
        const activeRow = await driver.devMachinesRowByName(activeLabel);
        expect(activeRow?.status).toBe("Active");
        expect(activeRow?.runners).toBe("1 active / 1 total");
        expect(activeRow?.actions).toEqual(["Rotate", "Revoke", "Delete"]);
        expect(activeRow?.lastHeartbeat).not.toBe("Never");
        expect(activeRow?.subline.startsWith("id ")).toBe(true);

        const offlineRow = await driver.devMachinesRowByName(offlineLabel);
        expect(offlineRow?.status).toBe("Offline");
        expect(offlineRow?.runners).toBe("0 active / 1 total");
        expect(offlineRow?.actions).toEqual(["Rotate", "Revoke", "Delete"]);
        expect(offlineRow?.subline).toBe(offlineHost);

        const registeredRow = await driver.devMachinesRowByName(registeredLabel);
        expect(registeredRow?.status).toBe("Registered");
        expect(registeredRow?.runners).toBe("0 active / 0 total");
        expect(registeredRow?.actions).toEqual(["Rotate", "Revoke", "Delete"]);
        expect(registeredRow?.lastSeen).toBe("Never");
        expect(registeredRow?.lastHeartbeat).toBe("Never");

        const revokedRow = await driver.devMachinesRowByName(revokedLabel);
        expect(revokedRow?.status).toBe("Revoked");
        expect(revokedRow?.actions).toEqual(["Delete"]);
      });

      await test.step("the server list carries the same machines and counts", async () => {
        const rows = await serverDevMachinesListMachines(wsId, session);
        const byId = new Map(rows.map((row) => [row.id, row]));
        expect(byId.get(active.id)?.onlineRunnerCount).toBe(1);
        expect(byId.get(active.id)?.runnerCount).toBe(1);
        expect(byId.get(offline.id)?.onlineRunnerCount).toBe(0);
        expect(byId.get(offline.id)?.runnerCount).toBe(1);
        expect(byId.get(registered.id)?.runnerCount).toBe(0);
        expect(byId.get(revoked.id)?.revokedAt).not.toBeNull();
      });
    } finally {
      await serverDevMachinesCleanupRunner(onlineRunner.id);
      await serverDevMachinesCleanupRunner(offlineRunner.id);
      await serverDevMachinesCleanupMachine(active.id);
      await serverDevMachinesCleanupMachine(offline.id);
      await serverDevMachinesCleanupMachine(registered.id);
      await serverDevMachinesCleanupMachine(revoked.id);
    }
  }
);

test(
  specTitle(["RUN-037"], "machines list states and refresh cadence"),
  { tag: specTags(["RUN-037"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDevMachinesWorkspaceId(seed.workspaceSlug);
    const label = tag("list-states");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await test.step("an empty list renders the empty state", async () => {
        await driver.devMachinesStubListOnce([]);
        await driver.devMachinesOpen(seed.workspaceSlug);
        await expect.poll(() => driver.devMachinesEmptyVisible(), { timeout: 15_000 }).toBe(true);
      });

      await test.step("a slow list renders the loading state, then recovers", async () => {
        await driver.devMachinesDelayListOnce(5000);
        const opened = driver.devMachinesOpen(seed.workspaceSlug);
        await expect.poll(() => driver.devMachinesLoadingVisible(), { timeout: 10_000 }).toBe(true);
        await opened;
        await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();
      });

      await test.step("a failed list renders the error state, then recovers", async () => {
        await driver.devMachinesFailListOnce();
        await driver.devMachinesOpen(seed.workspaceSlug);
        await expect.poll(() => driver.devMachinesErrorVisible(), { timeout: 15_000 }).toBe(true);
        await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 20_000 }).not.toBeNull();
      });

      await test.step("the list re-polls without navigation", async () => {
        expect(await driver.devMachinesListPollCount(11_000)).toBeGreaterThanOrEqual(2);
      });

      await test.step("the server still lists the planted machine", async () => {
        const rows = await serverDevMachinesListMachines(wsId, session);
        expect(rows.map((row) => row.id)).toContain(machine.id);
      });
    } finally {
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-038"], "rotate warns, closes, re-fetches and invalidates tokens"),
  { tag: specTags(["RUN-038"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDevMachinesWorkspaceId(seed.workspaceSlug);
    const label = tag("rotate-ok");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: machine.id,
      name: tag("rotate-ok-runner"),
      status: "online",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await test.step("the confirm warns that runners stop connecting", async () => {
        await driver.devMachinesOpenRotate(label);
        const modal = await driver.devMachinesModal();
        expect(modal?.title).toContain("Rotate");
        expect(modal?.body).toContain("invalidated");
        expect(modal?.body).toContain("stop connecting");
        expect(modal?.body).toContain("auth login");
        expect(modal?.confirmLabel).toBe("Rotate");
      });

      await test.step("confirming closes the modal and re-fetches the row", async () => {
        await driver.devMachinesModalConfirm();
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 15_000 }).toBe(false);
        await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();
      });

      await test.step("tokens and sessions are invalidated while the machine stays", async () => {
        expect(await serverDevMachinesTokenRevocations(machine.id)).toEqual([true]);
        expect(await serverDevMachinesSessionRevocations(runner.id)).toEqual([true]);
        const rows = await serverDevMachinesListMachines(wsId, session);
        expect(rows.find((row) => row.id === machine.id)?.revokedAt).toBeNull();
        const detail = await serverDevMachinesRunnerDetail(runner.id, session);
        expect(detail.detail?.status).toBe("online");
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-038"], "rotate blocks dismissal while in flight"),
  { tag: specTags(["RUN-038"]) },
  async ({ driver, seed }) => {
    const label = tag("rotate-busy");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await driver.devMachinesOpenRotate(label);
      await driver.devMachinesDelayActionOnce(8000);
      await driver.devMachinesModalConfirm();
      await driver.devMachinesModalPressEscape();
      await driver.devMachinesModalCancel();
      expect(await driver.devMachinesModalVisible()).toBe(true);

      await test.step("the delayed request still applies and closes the modal", async () => {
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 20_000 }).toBe(false);
        expect(await serverDevMachinesTokenRevocations(machine.id)).toEqual([true]);
      });
    } finally {
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-038"], "rotate failure toasts and keeps the modal open"),
  { tag: specTags(["RUN-038"]) },
  async ({ driver, seed }) => {
    const label = tag("rotate-fail");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await driver.devMachinesOpenRotate(label);
      await driver.devMachinesFailActionOnce();
      await driver.devMachinesModalConfirm();

      await test.step("an error toast shows and the modal stays open", async () => {
        await expect.poll(() => driver.devMachinesLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect(await driver.devMachinesLastToast()).toContain("Error");
        expect(await driver.devMachinesModalVisible()).toBe(true);
      });

      await test.step("no token was invalidated", async () => {
        expect(await serverDevMachinesTokenRevocations(machine.id)).toEqual([false]);
      });
    } finally {
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-039"], "revoke warns, revokes runners, row stays revoked with delete only"),
  { tag: specTags(["RUN-039"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDevMachinesWorkspaceId(seed.workspaceSlug);
    const label = tag("revoke-ok");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: machine.id,
      name: tag("revoke-ok-runner"),
      status: "online",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await test.step("the confirm warns the revoke is permanent", async () => {
        await driver.devMachinesOpenRevoke(label);
        const modal = await driver.devMachinesModal();
        expect(modal?.title).toContain("Revoke");
        expect(modal?.body).toContain("permanently");
        expect(modal?.body).toContain("invalidates");
        expect(modal?.body).toContain("revokes runners");
        expect(modal?.confirmLabel).toBe("Revoke");
      });

      await test.step("confirming leaves a revoked row with only delete", async () => {
        await driver.devMachinesModalConfirm();
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 15_000 }).toBe(false);
        await expect
          .poll(() => driver.devMachinesRowByName(label).then((row) => row?.status ?? null), { timeout: 15_000 })
          .toBe("Revoked");
        expect((await driver.devMachinesRowByName(label))?.actions).toEqual(["Delete"]);
      });

      await test.step("the machine, its tokens, runners and sessions are revoked", async () => {
        const rows = await serverDevMachinesListMachines(wsId, session);
        expect(rows.find((row) => row.id === machine.id)?.revokedAt).not.toBeNull();
        expect(await serverDevMachinesTokenRevocations(machine.id)).toEqual([true]);
        expect(await serverDevMachinesSessionRevocations(runner.id)).toEqual([true]);
        const detail = await serverDevMachinesRunnerDetail(runner.id, session);
        expect(detail.detail?.status).toBe("revoked");
        expect(detail.detail?.revokedAt).not.toBeNull();
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-039"], "revoke blocks dismissal in flight and toasts on failure"),
  { tag: specTags(["RUN-039"]) },
  async ({ driver, seed }) => {
    const busyLabel = tag("revoke-busy");
    const failLabel = tag("revoke-fail");
    const busy = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: busyLabel,
    });
    const failing = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: failLabel,
    });
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: busy.id,
      name: tag("revoke-busy-runner"),
      status: "online",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(busyLabel), { timeout: 15_000 }).not.toBeNull();

      await test.step("escape and cancel cannot dismiss the modal in flight", async () => {
        await driver.devMachinesOpenRevoke(busyLabel);
        await driver.devMachinesDelayActionOnce(8000);
        await driver.devMachinesModalConfirm();
        await driver.devMachinesModalPressEscape();
        await driver.devMachinesModalCancel();
        expect(await driver.devMachinesModalVisible()).toBe(true);
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 20_000 }).toBe(false);
        await expect
          .poll(() => driver.devMachinesRowByName(busyLabel).then((row) => row?.status ?? null), { timeout: 15_000 })
          .toBe("Revoked");
      });

      await test.step("a failed revoke toasts and revokes nothing", async () => {
        await driver.devMachinesOpenRevoke(failLabel);
        await driver.devMachinesFailActionOnce();
        await driver.devMachinesModalConfirm();
        await expect.poll(() => driver.devMachinesLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect(await driver.devMachinesLastToast()).toContain("Error");
        expect(await driver.devMachinesModalVisible()).toBe(true);
        expect(await serverDevMachinesTokenRevocations(failing.id)).toEqual([false]);
        expect(await serverDevMachinesMachineExists(failing.id)).toBe(true);
      });

      await test.step("the delayed revoke applied server-side", async () => {
        expect(await serverDevMachinesTokenRevocations(busy.id)).toEqual([true]);
        const detail = await serverDevMachinesRunnerDetail(runner.id, await signInSession(seed.email, seed.password));
        expect(detail.detail?.status).toBe("revoked");
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupMachine(busy.id);
      await serverDevMachinesCleanupMachine(failing.id);
    }
  }
);

test(
  specTitle(["RUN-040"], "delete warns, drops the row and cascades teardown"),
  { tag: specTags(["RUN-040"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDevMachinesWorkspaceId(seed.workspaceSlug);
    const label = tag("delete-ok");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label,
    });
    const first = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: machine.id,
      name: tag("delete-ok-first"),
      status: "online",
    });
    const second = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: machine.id,
      name: tag("delete-ok-second"),
      status: "offline",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).not.toBeNull();

      await test.step("the confirm warns while noting the agent binary stays", async () => {
        await driver.devMachinesDeleteSpyStart();
        await driver.devMachinesOpenDelete(label);
        const modal = await driver.devMachinesModal();
        expect(modal?.title).toContain("Delete");
        expect(modal?.body).toContain("invalidated");
        expect(modal?.body).toContain("torn down");
        expect(modal?.body).toContain("disappears");
        expect(modal?.body).toContain("does not uninstall");
        expect(modal?.confirmLabel).toBe("Delete");
      });

      await test.step("confirming removes the row with local teardown cascading", async () => {
        await driver.devMachinesModalConfirm();
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 15_000 }).toBe(false);
        await expect.poll(() => driver.devMachinesRowByName(label), { timeout: 15_000 }).toBeNull();
        const deletes = await driver.devMachinesDeleteSpyUrls();
        await driver.devMachinesDeleteSpyStop();
        expect(deletes.length).toBeGreaterThanOrEqual(1);
        expect(deletes[deletes.length - 1]).toContain("purge_local=true");
      });

      await test.step("the machine and its runners are gone server-side", async () => {
        expect(await serverDevMachinesMachineExists(machine.id)).toBe(false);
        const rows = await serverDevMachinesListMachines(wsId, session);
        expect(rows.map((row) => row.id)).not.toContain(machine.id);
        expect((await serverDevMachinesRunnerDetail(first.id, session)).status).toBe(404);
        expect((await serverDevMachinesRunnerDetail(second.id, session)).status).toBe(404);
      });
    } finally {
      await driver.devMachinesDeleteSpyStop().catch(() => undefined);
      await serverDevMachinesCleanupRunner(first.id);
      await serverDevMachinesCleanupRunner(second.id);
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-040"], "delete blocks dismissal in flight and toasts on failure"),
  { tag: specTags(["RUN-040"]) },
  async ({ driver, seed }) => {
    const busyLabel = tag("delete-busy");
    const failLabel = tag("delete-fail");
    const busy = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: busyLabel,
    });
    const failing = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: failLabel,
    });
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: busy.id,
      name: tag("delete-busy-runner"),
      status: "offline",
    });
    try {
      await openSignedInMachines(driver, seed);
      await expect.poll(() => driver.devMachinesRowByName(busyLabel), { timeout: 15_000 }).not.toBeNull();

      await test.step("escape and cancel cannot dismiss the modal in flight", async () => {
        await driver.devMachinesOpenDelete(busyLabel);
        await driver.devMachinesDelayActionOnce(8000);
        await driver.devMachinesModalConfirm();
        await driver.devMachinesModalPressEscape();
        await driver.devMachinesModalCancel();
        expect(await driver.devMachinesModalVisible()).toBe(true);
        await expect.poll(() => driver.devMachinesModalVisible(), { timeout: 20_000 }).toBe(false);
        await expect.poll(() => driver.devMachinesRowByName(busyLabel), { timeout: 15_000 }).toBeNull();
      });

      await test.step("a failed delete toasts and deletes nothing", async () => {
        await driver.devMachinesOpenDelete(failLabel);
        await driver.devMachinesFailActionOnce();
        await driver.devMachinesModalConfirm();
        await expect.poll(() => driver.devMachinesLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect(await driver.devMachinesLastToast()).toContain("Error");
        expect(await driver.devMachinesModalVisible()).toBe(true);
        expect(await serverDevMachinesMachineExists(failing.id)).toBe(true);
      });

      await test.step("the delayed delete applied server-side", async () => {
        expect(await serverDevMachinesMachineExists(busy.id)).toBe(false);
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupMachine(busy.id);
      await serverDevMachinesCleanupMachine(failing.id);
    }
  }
);

test(
  specTitle(["RUN-041"], "install cards show platform commands with copy-confirm"),
  { tag: specTags(["RUN-041"]) },
  async ({ driver, seed }) => {
    await openSignedInMachines(driver, seed);

    await test.step("all three platform cards render with their commands", async () => {
      const cards = await driver.devMachinesInstallCards();
      expect(cards.map((card) => card.label)).toEqual(["macOS / Linux", "Windows (PowerShell)", "Windows (MSI)"]);
      const byLabel = new Map(cards.map((card) => [card.label, card]));
      expect(byLabel.get("macOS / Linux")?.command).toContain("install.sh");
      expect(byLabel.get("Windows (PowerShell)")?.command).toContain("install.ps1");
      expect(byLabel.get("Windows (MSI)")?.command.toLowerCase()).toContain("msi");
      expect(byLabel.get("macOS / Linux")?.downloadHref).toBeNull();
      expect(byLabel.get("Windows (PowerShell)")?.downloadHref).toBeNull();
      const msiHref = byLabel.get("Windows (MSI)")?.downloadHref ?? "";
      expect(msiHref).toContain("releases/latest/download");
      expect(msiHref.endsWith(".msi")).toBe(true);
    });

    await test.step("the prerequisite note points at PATH and doctor", async () => {
      const prereq = await driver.devMachinesInstallPrereq();
      expect(prereq).toContain("PATH");
      expect(prereq).toContain("doctor");
    });

    await test.step("copying writes the displayed command and confirms transiently", async () => {
      for (const label of ["macOS / Linux", "Windows (PowerShell)", "Windows (MSI)"]) {
        const shown = (await driver.devMachinesInstallCards()).find((card) => card.label === label)?.command ?? "";
        expect(shown.length).toBeGreaterThan(0);
        await driver.devMachinesInstallCopy(label);
        expect(await driver.devMachinesReadClipboard()).toBe(shown);
        await expect.poll(() => driver.devMachinesInstallCopyState(label), { timeout: 10_000 }).toBe("Copied!");
        await expect.poll(() => driver.devMachinesInstallCopyState(label), { timeout: 10_000 }).toBe("Copy command");
      }
    });
  }
);

test(
  specTitle(["RUN-041"], "copy failure toasts instead of confirming"),
  { tag: specTags(["RUN-041"]) },
  async ({ driver, seed }) => {
    await openSignedInMachines(driver, seed);

    await driver.devMachinesInstallBreakClipboard();
    await driver.devMachinesInstallCopy("macOS / Linux");
    await expect.poll(() => driver.devMachinesLastToast(), { timeout: 15_000 }).not.toBeNull();
    expect((await driver.devMachinesLastToast())?.toLowerCase()).toContain("clipboard");
    expect(await driver.devMachinesInstallCopyState("macOS / Linux")).toBe("Copy command");
  }
);

test(
  specTitle(["RUN-042"], "runner detail shows the full metadata grid"),
  { tag: specTags(["RUN-042"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const machineLabel = tag("detail machine");
    const machine = await serverDevMachinesPlantMachine({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      label: machineLabel,
    });
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: machine.id,
      name: tag("detail rich"),
      status: "online",
      os: "linux",
      arch: "arm64",
      runnerVersion: "9.9.9-parity",
      workingDir: "/tmp/parity-wd",
      capabilities: ["parity-cap-a", "parity-cap-b"],
      lastHeartbeatAgeSecs: 60,
    });
    try {
      const server = (await serverDevMachinesRunnerDetail(runner.id, session)).detail;
      expect(server?.status).toBe("online");

      await openSignedInDetail(driver, seed, runner.id);

      await test.step("the header shows the name and status badge", async () => {
        await expect.poll(() => driver.runnerDetailHeader(), { timeout: 15_000 }).not.toBeNull();
        expect(await driver.runnerDetailHeader()).toEqual({ name: runner.name, status: "online" });
      });

      await test.step("every metadata row matches the server record", async () => {
        const meta = await driver.runnerDetailMeta();
        expect(metaValue(meta, "Runner ID")).toBe(runner.id);
        expect(metaValue(meta, "Pod")).toBe(server?.podName);
        expect(metaValue(meta, "Project")).toBe(server?.podProject);
        expect(metaValue(meta, "Dev machine")).toBe(machineLabel);
        expect(metaValue(meta, "OS / Arch")).toBe("linux / arm64");
        expect(metaValue(meta, "Version")).toBe("9.9.9-parity");
        expect(metaValue(meta, "Working directory")).toBe("/tmp/parity-wd");
        expect(metaValue(meta, "Protocol version")).toBe(String(server?.protocolVersion));
        expect(metaValue(meta, "Capabilities")).toContain("parity-cap-a");
        expect(metaValue(meta, "Capabilities")).toContain("parity-cap-b");
        expect(metaValue(meta, "Connection")).toBe("—");
        expect(metaValue(meta, "Owner")).toBe(server?.owner);
        expect(metaValue(meta, "Last heartbeat")).not.toBe("—");
        expect(metaValue(meta, "Enrolled at")).not.toBe("—");
        expect(metaValue(meta, "Enrolled at")).not.toBe("Pending enrollment");
        expect(metaValue(meta, "Created at")).not.toBe("—");
        expect(metaValue(meta, "Updated at")).not.toBe("—");
        expect(metaValue(meta, "Revoked at")).toBeNull();
        expect(metaValue(meta, "Revoked reason")).toBeNull();
      });

      await test.step("the back link returns to the workspace list", async () => {
        expect(await driver.runnerDetailBackHref()).toBe(`/${seed.workspaceSlug}/runners`);
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupMachine(machine.id);
    }
  }
);

test(
  specTitle(["RUN-042"], "detail fallbacks, project scope, into-chat, states and cadence"),
  { tag: specTags(["RUN-042"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: null,
      name: tag("Detail bare"),
      status: "offline",
      enrolled: "none",
    });
    const revokedRunner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: null,
      name: tag("Detail revoked"),
      status: "revoked",
      enrolled: "none",
      revokedReason: "parity-probe-revoke",
    });
    try {
      await openSignedInDetail(driver, seed, runner.id);

      await test.step("missing fields render dash and pending-enrolment fallbacks", async () => {
        await expect.poll(() => driver.runnerDetailHeader(), { timeout: 15_000 }).not.toBeNull();
        const meta = await driver.runnerDetailMeta();
        expect(metaValue(meta, "Runner ID")).toBe(runner.id);
        expect(metaValue(meta, "Pod")).not.toBe("—");
        expect(metaValue(meta, "Project")).not.toBe("—");
        expect(metaValue(meta, "Dev machine")).toBe("—");
        expect(metaValue(meta, "OS / Arch")).toBe("—");
        expect(metaValue(meta, "Version")).toBe("—");
        expect(metaValue(meta, "Working directory")).toBe("—");
        expect(metaValue(meta, "Capabilities")).toBe("—");
        expect(metaValue(meta, "Connection")).toBe("—");
        expect(metaValue(meta, "Last heartbeat")).toBe("—");
        expect(metaValue(meta, "Enrolled at")).toBe("Pending enrollment");
      });

      await test.step("a revoked runner shows its revocation rows", async () => {
        await driver.runnerDetailOpen(seed.workspaceSlug, revokedRunner.id);
        await expect
          .poll(() => driver.runnerDetailHeader().then((header) => header?.name ?? null), { timeout: 15_000 })
          .toBe(revokedRunner.name);
        const meta = await driver.runnerDetailMeta();
        expect(metaValue(meta, "Revoked at")).not.toBe("—");
        expect(metaValue(meta, "Revoked reason")).toContain("parity-probe-revoke");
      });

      await test.step("the project twin loads with its own back link", async () => {
        await driver.runnerDetailOpen(seed.workspaceSlug, runner.id, seed.projectId);
        await expect
          .poll(() => driver.runnerDetailHeader().then((header) => header?.name ?? null), { timeout: 15_000 })
          .toBe(runner.name);
        expect(await driver.runnerDetailBackHref()).toBe(`/${seed.workspaceSlug}/projects/${seed.projectId}/runners`);
      });

      await test.step("the into-chat control lands on the runner chat", async () => {
        await driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
        await expect.poll(() => driver.runnerDetailHeader(), { timeout: 15_000 }).not.toBeNull();
        await driver.runnerDetailOpenChat();
        expect(await driver.currentPath()).toContain(`chat/${runner.id}`);
      });

      await test.step("loading and error states render around the fetch", async () => {
        await driver.runnerDetailDelayOnce(5000);
        const opened = driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
        await expect.poll(() => driver.runnerDetailState(), { timeout: 10_000 }).toBe("loading");
        await opened;
        await expect.poll(() => driver.runnerDetailState(), { timeout: 15_000 }).toBe("loaded");

        await driver.runnerDetailFailOnce();
        await driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
        await expect.poll(() => driver.runnerDetailState(), { timeout: 15_000 }).toBe("error");
        expect(await driver.runnerDetailHeader()).toBeNull();
        await expect.poll(() => driver.runnerDetailState(), { timeout: 25_000 }).toBe("loaded");
      });

      await test.step("the page re-polls without navigation", async () => {
        expect(await driver.runnerDetailPollCount(runner.id, 11_000)).toBeGreaterThanOrEqual(2);
      });

      await test.step("the server record matches the bare fixture", async () => {
        const detail = (await serverDevMachinesRunnerDetail(runner.id, session)).detail;
        expect(detail?.status).toBe("offline");
        expect(detail?.enrolledAt).toBeNull();
        expect(detail?.os).toBe("");
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
      await serverDevMachinesCleanupRunner(revokedRunner.id);
    }
  }
);

test(
  specTitle(["RUN-043"], "activity badge derives all six outcomes"),
  { tag: specTags(["RUN-043"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    const cases: { name: string; badge: string; live: Parameters<typeof serverDevMachinesPlantLiveState>[1] | null }[] =
      [
        { name: tag("badge-unknown"), badge: "Unknown", live: null },
        {
          name: tag("badge-dead"),
          badge: "Subprocess dead",
          live: { lastEventAgeSecs: 5, agentPid: 4242, subprocessAlive: false, approvalsPending: 0 },
        },
        {
          name: tag("badge-awaiting"),
          badge: "Awaiting approval",
          live: { lastEventAgeSecs: 5, agentPid: 4242, subprocessAlive: true, approvalsPending: 2 },
        },
        {
          name: tag("badge-active"),
          badge: "Active",
          live: { lastEventAgeSecs: 3, agentPid: 4242, subprocessAlive: true, approvalsPending: 0 },
        },
        {
          name: tag("badge-thinking"),
          badge: "Thinking",
          live: { lastEventAgeSecs: 100, agentPid: 4242, subprocessAlive: true, approvalsPending: 0 },
        },
        {
          name: tag("badge-stalled"),
          badge: "Stalled",
          live: { lastEventAgeSecs: 500, agentPid: 4242, subprocessAlive: true, approvalsPending: 0 },
        },
      ];
    const ids: string[] = [];
    try {
      for (const entry of cases) {
        const runner = await serverDevMachinesPlantRunner({
          ownerEmail: seed.email,
          workspaceSlug: seed.workspaceSlug,
          projectId: seed.projectId,
          machineId: null,
          name: entry.name,
          status: "offline",
        });
        ids.push(runner.id);
        if (entry.live === null) {
          await serverDevMachinesClearLiveState(runner.id);
        } else {
          // Plant just before opening so event-age thresholds stay exact.
          await serverDevMachinesPlantLiveState(runner.id, entry.live);
        }
        await test.step(`badge shows ${entry.badge}`, async () => {
          await driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
          await expect.poll(() => driver.runnerActivityBadge(), { timeout: 15_000 }).toBe(entry.badge);
        });
      }

      await test.step("the server live state round-trips the planted snapshot", async () => {
        const detail = (await serverDevMachinesRunnerDetail(ids[3] ?? "", session)).detail;
        expect(detail?.liveState?.agentPid).toBe(4242);
        expect(detail?.liveState?.approvalsPending).toBe(0);
      });
    } finally {
      for (const id of ids) {
        await serverDevMachinesCleanupRunner(id);
      }
    }
  }
);

test(
  specTitle(["RUN-043"], "telemetry grid, compact numbers and the local tick"),
  { tag: specTags(["RUN-043"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const runner = await serverDevMachinesPlantRunner({
      ownerEmail: seed.email,
      workspaceSlug: seed.workspaceSlug,
      projectId: seed.projectId,
      machineId: null,
      name: tag("telemetry"),
      status: "offline",
    });
    try {
      // Open first: sign-in plus navigation can take a minute under load,
      // so age-sensitive snapshots are planted only once the page polls.
      await openSignedInDetail(driver, seed, runner.id);
      await expect.poll(() => driver.runnerActivityBadge(), { timeout: 15_000 }).toBe("Unknown");

      await test.step("a missing snapshot renders dash fallbacks", async () => {
        const grid = await driver.runnerActivityTelemetry();
        expect(metaValue(grid, "Last activity")).toBe("—");
        expect(metaValue(grid, "Last event")).toBe("—");
        expect(metaValue(grid, "Agent PID")).toBe("—");
        expect(metaValue(grid, "Subprocess alive")).toBe("—");
        expect(metaValue(grid, "Approvals")).toBe("0");
        expect(metaValue(grid, "Tokens")).toBe("—");
        expect(metaValue(grid, "Model")).toBe("—");
        expect(metaValue(grid, "Turn")).toBe("—");
      });

      await test.step("the grid shows every telemetry scalar compactly", async () => {
        await serverDevMachinesPlantLiveState(runner.id, {
          lastEventAgeSecs: 5,
          lastEventKind: "parity-probe-kind",
          lastEventSummary: "parity probe summary",
          agentPid: 4242,
          subprocessAlive: true,
          approvalsPending: 3,
          inputTokens: 1500,
          outputTokens: 2500000,
          totalTokens: 2501500,
          llmModel: "parity-model-9",
          turnCount: 7,
        });
        await expect
          .poll(() => driver.runnerActivityTelemetry().then((grid) => metaValue(grid, "Last activity")), {
            timeout: 15_000,
          })
          .toMatch(/^\d+s ago$/);
        const grid = await driver.runnerActivityTelemetry();
        expect(metaValue(grid, "Last activity")).toMatch(/^\d+s ago$/);
        expect(metaValue(grid, "Last event")).toBe("parity-probe-kind");
        expect(metaValue(grid, "Agent PID")).toBe("4242");
        expect(metaValue(grid, "Subprocess alive")).toBe("yes");
        expect(metaValue(grid, "Approvals")).toBe("3");
        expect(metaValue(grid, "Tokens")).toBe("2.5M");
        expect(metaValue(grid, "Model")).toBe("parity-model-9");
        expect(metaValue(grid, "Turn")).toBe("7");
      });

      await test.step("thousands compact to k", async () => {
        await serverDevMachinesPlantLiveState(runner.id, {
          lastEventAgeSecs: 5,
          agentPid: 4242,
          subprocessAlive: true,
          totalTokens: 1500,
        });
        await driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
        await expect
          .poll(() => driver.runnerActivityTelemetry().then((grid) => metaValue(grid, "Tokens")), { timeout: 15_000 })
          .toBe("1.5k");
      });

      await test.step("the panel ages visibly without server refetches", async () => {
        await serverDevMachinesPlantLiveState(runner.id, {
          lastEventAgeSecs: 5,
          agentPid: 4242,
          subprocessAlive: true,
        });
        await driver.runnerDetailOpen(seed.workspaceSlug, runner.id);
        await expect.poll(() => driver.runnerActivityBadge(), { timeout: 15_000 }).toBe("Active");
        expect(await driver.runnerActivityAgingObserved()).toBe(true);
      });

      await test.step("the server live state round-trips the snapshot", async () => {
        await serverDevMachinesPlantLiveState(runner.id, {
          lastEventAgeSecs: 5,
          lastEventKind: "parity-probe-kind",
          agentPid: 4242,
          subprocessAlive: true,
          approvalsPending: 3,
          totalTokens: 2501500,
          llmModel: "parity-model-9",
          turnCount: 7,
        });
        const detail = (await serverDevMachinesRunnerDetail(runner.id, session)).detail;
        expect(detail?.liveState?.lastEventKind).toBe("parity-probe-kind");
        expect(detail?.liveState?.totalTokens).toBe(2501500);
        expect(detail?.liveState?.llmModel).toBe("parity-model-9");
        expect(detail?.liveState?.turnCount).toBe(7);
      });
    } finally {
      await serverDevMachinesCleanupRunner(runner.id);
    }
  }
);
