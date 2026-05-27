/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Check, Copy, Cpu, Download, Laptop, Terminal } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button, getButtonStyling } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
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

// We don't render a runners list on this page, so there's nothing to refetch
// on success — the modal still requires the callback. The AI Agents page
// itself shows the newly enrolled row.
const noopOnCreated = () => {};

const AiDevMachinesPage = observer(function AiDevMachinesPage() {
  const { currentWorkspace } = useWorkspace();
  const { t } = useTranslation();

  const workspaceId = currentWorkspace?.id;
  const workspaceSlug = currentWorkspace?.slug;
  const pageTitle = currentWorkspace?.name
    ? t("ai_dev_machines.page_title", { workspace: currentWorkspace.name })
    : t("ai_dev_machines.title");

  const [addOpen, setAddOpen] = useState(false);

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

      {/* Add AI Agent runner — same modal used on the AI Agents page */}
      <section className="rounded-md border border-subtle p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="text-13 font-medium text-primary">{t("ai_dev_machines.add_runner.heading")}</div>
            <p className="mt-1 text-13 text-secondary">{t("ai_dev_machines.add_runner.body")}</p>
          </div>
          <Button onClick={() => setAddOpen(true)} disabled={!workspaceId}>
            {t("ai_dev_machines.add_runner.cta")}
          </Button>
        </div>
      </section>

      {workspaceId && workspaceSlug && (
        <AddRunnerModal
          isOpen={addOpen}
          onClose={() => setAddOpen(false)}
          workspaceId={workspaceId}
          workspaceSlug={workspaceSlug}
          onCreated={noopOnCreated}
        />
      )}
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
