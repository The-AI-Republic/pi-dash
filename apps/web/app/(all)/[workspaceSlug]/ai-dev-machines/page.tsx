/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Ban, Check, Copy, Cpu, Download, Laptop, RotateCw, Terminal } from "lucide-react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { Button, getButtonStyling } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { RunnerService } from "@pi-dash/services";
import type { IDevMachine } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";
import { AlertModalCore, Badge } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { AddRunnerModal } from "@/components/runners/add-runner-modal";
import { useWorkspace } from "@/hooks/store/use-workspace";

// Installers point at the GitHub Releases ``latest`` channel. The wrapper
// installers (install.sh / install.ps1) download the cargo-dist installer,
// drop ``pidash`` on PATH, and immediately start device-code login. See
// runner/README.md for the full matrix (tag-pinned, bare installers, etc.).
const INSTALL_CMD_UNIX = `curl --proto '=https' --tlsv1.2 -LsSf \\
  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh`;
const INSTALL_CMD_WINDOWS = `irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex`;
const WINDOWS_MSI_URL =
  "https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi";
const INSTALL_CMD_WINDOWS_MSI = `$ProgressPreference = "SilentlyContinue"
$msi = Join-Path $env:TEMP "pidash-x86_64-pc-windows-msvc.msi"
Invoke-WebRequest -Uri "${WINDOWS_MSI_URL}" -OutFile $msi
Start-Process msiexec.exe -Wait -ArgumentList "/i \`"$msi\`""`;

const service = new RunnerService();

type DevMachineStatus = "active" | "offline" | "registered" | "revoked";

const DEV_MACHINE_STATUS_BADGE_VARIANT: Record<DevMachineStatus, TBadgeVariant> = {
  active: "accent-success",
  offline: "accent-neutral",
  registered: "accent-primary",
  revoked: "accent-warning",
};

function getDevMachineStatus(machine: IDevMachine): DevMachineStatus {
  if (machine.revoked_at) return "revoked";
  if (machine.online_runner_count > 0) return "active";
  if (machine.runner_count > 0) return "offline";
  return "registered";
}

function shortMachineId(id: string): string {
  return id.slice(0, 8);
}

function machineDisplayName(machine: IDevMachine): string {
  return machine.label || machine.host_label || shortMachineId(machine.id);
}

function formatDateTime(value: string | null, fallback: string): string {
  return value ? new Date(value).toLocaleString() : fallback;
}

const AiDevMachinesPage = observer(function AiDevMachinesPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();

  const workspaceId = currentWorkspace?.id;
  const workspaceSlug = currentWorkspace?.slug;
  const pageTitle = currentWorkspace?.name
    ? t("ai_dev_machines.page_title", { workspace: currentWorkspace.name })
    : t("ai_dev_machines.title");

  const [addOpen, setAddOpen] = useState(false);
  const [rotateMachine, setRotateMachine] = useState<IDevMachine | null>(null);
  const [revokeMachine, setRevokeMachine] = useState<IDevMachine | null>(null);
  const [rotating, setRotating] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const {
    data: devMachines,
    error: devMachinesError,
    mutate: mutateDevMachines,
  } = useSWR<IDevMachine[]>(
    workspaceId ? ["dev-machines", workspaceId] : null,
    () => service.listDevMachines(workspaceId!),
    { refreshInterval: 5_000 }
  );

  async function confirmRotateMachine() {
    if (!rotateMachine || !workspaceId) return;
    setRotating(true);
    try {
      await service.rotateDevMachine(rotateMachine.id, workspaceId);
      setRotateMachine(null);
      mutateDevMachines();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("ai_dev_machines.list.rotate_failed"),
      });
    } finally {
      setRotating(false);
    }
  }

  async function confirmRevokeMachine() {
    if (!revokeMachine || !workspaceId) return;
    setRevoking(true);
    try {
      await service.revokeDevMachine(revokeMachine.id, workspaceId);
      setRevokeMachine(null);
      mutateDevMachines();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: err?.error ?? t("ai_dev_machines.list.revoke_failed"),
      });
    } finally {
      setRevoking(false);
    }
  }

  return (
    <div className="flex flex-col gap-8 p-6">
      <PageHead title={pageTitle} />

      <header>
        <h1 className="text-16 font-semibold text-primary">{t("ai_dev_machines.title")}</h1>
      </header>

      {/* Intro — what the CLI / daemon / runner each are */}
      <section className="flex flex-col gap-4">
        <h2 className="text-14 font-semibold text-primary">{t("ai_dev_machines.intro.heading")}</h2>
        <p className="text-13 text-secondary">{t("ai_dev_machines.intro.body")}</p>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <ConceptCard
            icon={<Terminal className="size-4" />}
            title={t("ai_dev_machines.intro.cli.title")}
            body={t("ai_dev_machines.intro.cli.body")}
          />
          <ConceptCard
            icon={<Cpu className="size-4" />}
            title={t("ai_dev_machines.intro.daemon.title")}
            body={t("ai_dev_machines.intro.daemon.body")}
          />
          <ConceptCard
            icon={<Laptop className="size-4" />}
            title={t("ai_dev_machines.intro.runner.title")}
            body={t("ai_dev_machines.intro.runner.body")}
          />
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h2 className="text-14 font-semibold text-primary">{t("ai_dev_machines.list.heading")}</h2>
            <p className="mt-1 text-13 text-secondary">{t("ai_dev_machines.list.body")}</p>
          </div>
          <Button onClick={() => setAddOpen(true)} disabled={!workspaceId}>
            {t("ai_dev_machines.list.add_runner")}
          </Button>
        </div>
        <div className="overflow-x-auto rounded-md border border-subtle">
          <table className="w-full text-13">
            <thead className="bg-layer-1 text-left text-secondary">
              <tr>
                <th className="px-3 py-2">{t("ai_dev_machines.list.columns.machine")}</th>
                <th className="px-3 py-2">{t("ai_dev_machines.list.columns.status")}</th>
                <th className="px-3 py-2">{t("ai_dev_machines.list.columns.runners")}</th>
                <th className="px-3 py-2">{t("ai_dev_machines.list.columns.last_seen")}</th>
                <th className="px-3 py-2">{t("ai_dev_machines.list.columns.last_heartbeat")}</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {devMachinesError && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-danger-primary">
                    {t("ai_dev_machines.list.load_failed")}
                  </td>
                </tr>
              )}
              {!devMachinesError && !devMachines && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                    {t("ai_dev_machines.list.loading")}
                  </td>
                </tr>
              )}
              {!devMachinesError &&
                (devMachines ?? []).map((machine) => {
                  const status = getDevMachineStatus(machine);
                  const name = machineDisplayName(machine);
                  const hostLabel = machine.host_label && machine.host_label !== name ? machine.host_label : "";
                  return (
                    <tr key={machine.id} className="border-t border-subtle">
                      <td className="px-3 py-2">
                        <div className="font-medium text-primary">{name}</div>
                        <div className="font-mono text-11 text-secondary">
                          {hostLabel || t("ai_dev_machines.list.machine_id", { id: shortMachineId(machine.id) })}
                        </div>
                      </td>
                      <td className="px-3 py-2">
                        <Badge variant={DEV_MACHINE_STATUS_BADGE_VARIANT[status]} size="sm">
                          {t(`ai_dev_machines.list.status.${status}`)}
                        </Badge>
                      </td>
                      <td className="px-3 py-2">
                        {t("ai_dev_machines.list.runner_count", {
                          active: machine.online_runner_count,
                          total: machine.runner_count,
                        })}
                      </td>
                      <td className="px-3 py-2">
                        {formatDateTime(machine.last_seen_at, t("ai_dev_machines.list.never"))}
                      </td>
                      <td className="px-3 py-2">
                        {formatDateTime(machine.last_heartbeat_at, t("ai_dev_machines.list.never"))}
                      </td>
                      <td className="px-3 py-2 text-right">
                        <div className="flex justify-end gap-2">
                          {!machine.revoked_at && (
                            <>
                              <Button
                                size="sm"
                                variant="secondary"
                                prependIcon={<RotateCw />}
                                onClick={() => setRotateMachine(machine)}
                              >
                                {t("ai_dev_machines.list.rotate")}
                              </Button>
                              <Button
                                size="sm"
                                variant="error-outline"
                                prependIcon={<Ban />}
                                onClick={() => setRevokeMachine(machine)}
                              >
                                {t("ai_dev_machines.list.revoke")}
                              </Button>
                            </>
                          )}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              {!devMachinesError && devMachines && devMachines.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-secondary">
                    {t("ai_dev_machines.list.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      {/* Install command — copy / paste */}
      <section className="flex flex-col gap-3">
        <h2 className="text-14 font-semibold text-primary">{t("ai_dev_machines.install.heading")}</h2>
        <p className="text-13 text-secondary">{t("ai_dev_machines.install.body")}</p>

        <InstallCommand label={t("ai_dev_machines.install.macos_linux_label")} command={INSTALL_CMD_UNIX} />
        <InstallCommand label={t("ai_dev_machines.install.windows_label")} command={INSTALL_CMD_WINDOWS} />
        <InstallCommand
          label={t("ai_dev_machines.install.windows_msi_label")}
          command={INSTALL_CMD_WINDOWS_MSI}
          href={WINDOWS_MSI_URL}
          hrefLabel={t("ai_dev_machines.install.download_msi")}
        />

        <p className="text-12 text-secondary">{t("ai_dev_machines.install.prereq")}</p>
      </section>

      {workspaceId && workspaceSlug && (
        <AddRunnerModal
          isOpen={addOpen}
          onClose={() => setAddOpen(false)}
          workspaceId={workspaceId}
          workspaceSlug={workspaceSlug}
        />
      )}
      <AlertModalCore
        isOpen={!!rotateMachine}
        handleClose={() => (rotating ? null : setRotateMachine(null))}
        handleSubmit={confirmRotateMachine}
        isSubmitting={rotating}
        title={t("ai_dev_machines.list.rotate_confirm_title")}
        content={t("ai_dev_machines.list.rotate_confirm_body")}
        variant="primary"
        primaryButtonText={{ default: t("ai_dev_machines.list.rotate"), loading: t("ai_dev_machines.list.rotate") }}
      />
      <AlertModalCore
        isOpen={!!revokeMachine}
        handleClose={() => (revoking ? null : setRevokeMachine(null))}
        handleSubmit={confirmRevokeMachine}
        isSubmitting={revoking}
        title={t("ai_dev_machines.list.revoke_confirm_title")}
        content={t("ai_dev_machines.list.revoke_confirm_body")}
        primaryButtonText={{ default: t("ai_dev_machines.list.revoke"), loading: t("ai_dev_machines.list.revoke") }}
      />
    </div>
  );
});

type ConceptCardProps = {
  icon: React.ReactNode;
  title: string;
  body: string;
};

function ConceptCard({ icon, title, body }: ConceptCardProps) {
  return (
    <div className="flex flex-col gap-2 rounded-md border border-subtle bg-layer-1 p-3">
      <div className="flex items-center gap-2 text-primary">
        {icon}
        <span className="text-13 font-medium">{title}</span>
      </div>
      <p className="text-12 text-secondary">{body}</p>
    </div>
  );
}

type InstallCommandProps = {
  label: string;
  command: string;
  href?: string;
  hrefLabel?: string;
};

function InstallCommand({ label, command, href, hrefLabel }: InstallCommandProps) {
  const { t } = useTranslation();
  const [justCopied, setJustCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: t("ai_dev_machines.install.copy_failed"),
      });
    }
  };

  return (
    <div className="rounded-md border border-subtle bg-layer-1 p-3">
      <div className="mb-2 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span className="text-12 font-medium text-secondary uppercase">{label}</span>
        <div className="flex flex-wrap items-center gap-2">
          {href && hrefLabel && (
            <a href={href} target="_blank" rel="noreferrer noopener" className={getButtonStyling("secondary", "sm")}>
              <Download className="size-3.5 shrink-0" />
              {hrefLabel}
            </a>
          )}
          <Button size="sm" variant="secondary" onClick={copy} prependIcon={justCopied ? <Check /> : <Copy />}>
            {justCopied ? t("ai_dev_machines.install.copied") : t("ai_dev_machines.install.copy_command")}
          </Button>
        </div>
      </div>
      <pre className="font-mono rounded border border-subtle bg-layer-2 p-2 text-11 whitespace-pre-wrap text-primary select-all">
        {command}
      </pre>
    </div>
  );
}

export default AiDevMachinesPage;
