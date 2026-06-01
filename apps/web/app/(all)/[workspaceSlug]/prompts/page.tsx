/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Link, useNavigate, useParams } from "react-router";
import useSWR from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IPromptTemplate } from "@pi-dash/types";
import { AlertModalCore, Badge, Button } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { usePromptTemplate } from "@/hooks/store/use-prompt-template";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";

/**
 * Prompt templates list. Shows:
 *   - The effective "Pi Dash default" (workspace=null) template.
 *   - The workspace-scoped override, if one is active.
 *
 * Admins see a "Customize" button when no override exists, and
 * "Edit" / "Revert to default" actions on any existing override. Members and
 * guests see the rows as read-only.
 */
const PromptsListPage = observer(function PromptsListPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { currentWorkspace } = useWorkspace();
  const { allowPermissions } = useUserPermissions();
  const navigate = useNavigate();
  const { t } = useTranslation();

  const promptStore = usePromptTemplate();

  const slug = workspaceSlug ?? "";
  const { data: templates, mutate } = useSWR<IPromptTemplate[]>(slug ? ["prompt-templates", slug] : null, () =>
    promptStore.fetchTemplates(slug)
  );

  const canEdit = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, slug);

  const [customizing, setCustomizing] = useState(false);
  const [archiveTarget, setArchiveTarget] = useState<IPromptTemplate | null>(null);
  const [archiving, setArchiving] = useState(false);

  const rows = templates ?? [];
  const workspaceOverride = rows.find((r) => !r.is_global_default);
  const globalDefault = rows.find((r) => r.is_global_default);

  async function handleCustomize() {
    if (!canEdit || customizing) return;
    setCustomizing(true);
    try {
      const created = await promptStore.createOverride(slug);
      mutate();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Workspace override created"),
        message: t("We copied the current Pi Dash default. Edit and save to customize it."),
      });
      navigate(`/${slug}/prompts/${created.id}`);
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: err?.error ?? t("Could not create the workspace override."),
      });
    } finally {
      setCustomizing(false);
    }
  }

  async function confirmArchive() {
    if (!archiveTarget) return;
    setArchiving(true);
    try {
      await promptStore.archiveTemplate(slug, archiveTarget.id);
      setArchiveTarget(null);
      mutate();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Reverted to Pi Dash default"),
        message: t("This workspace is back on the shared default template."),
      });
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Something went wrong"),
        message: err?.error ?? t("Could not revert the prompt."),
      });
    } finally {
      setArchiving(false);
    }
  }

  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} · ${t("Prompts")}` : t("Prompts");

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHead title={pageTitle} />

      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-16 font-semibold text-primary">{t("Prompts")}</h1>
          <p className="mt-1 text-13 text-secondary">{t("System prompt templates that get rendered against each issue before an agent run. Workspace admins can customize the default for this workspace.")}</p>
        </div>
        {canEdit && !workspaceOverride && (
          <Button onClick={handleCustomize} loading={customizing} disabled={customizing}>
            {t("Customize for this workspace")}
          </Button>
        )}
      </header>

      <section className="rounded-md border border-subtle">
        <table className="w-full text-13">
          <thead className="bg-layer-1 text-left text-secondary">
            <tr>
              <th className="px-3 py-2">{t("Name")}</th>
              <th className="px-3 py-2">{t("Scope")}</th>
              <th className="px-3 py-2">{t("Version")}</th>
              <th className="px-3 py-2">{t("Updated")}</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {workspaceOverride && (
              <TemplateRow
                key={workspaceOverride.id}
                template={workspaceOverride}
                slug={slug}
                canEdit={canEdit}
                onArchive={() => setArchiveTarget(workspaceOverride)}
              />
            )}
            {globalDefault && (
              <TemplateRow
                key={globalDefault.id}
                template={globalDefault}
                slug={slug}
                canEdit={canEdit}
                onArchive={null}
              />
            )}
            {rows.length === 0 && (
              <tr>
                <td colSpan={5} className="px-3 py-8 text-center text-secondary">
                  {t("No prompt templates available. The Pi Dash default will be seeded on the next migrate.")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <AlertModalCore
        isOpen={!!archiveTarget}
        handleClose={() => (archiving ? null : setArchiveTarget(null))}
        handleSubmit={confirmArchive}
        isSubmitting={archiving}
        title={t("Revert to the Pi Dash default?")}
        content={t("This archives your workspace-scoped template. New agent runs in this workspace will use the Pi Dash default until you create another override.")}
        primaryButtonText={{
          default: t("Revert"),
          loading: t("Revert"),
        }}
      />
    </div>
  );
});

type RowProps = {
  template: IPromptTemplate;
  slug: string;
  canEdit: boolean;
  onArchive: (() => void) | null;
};

function TemplateRow({ template, slug, canEdit, onArchive }: RowProps) {
  const { t } = useTranslation();
  return (
    <tr className="border-t border-subtle">
      <td className="px-3 py-2 font-medium text-primary">{template.name}</td>
      <td className="px-3 py-2">
        {template.is_global_default ? (
          <Badge variant="accent-neutral" size="sm">
            {t("Pi Dash default")}
          </Badge>
        ) : (
          <Badge variant="accent-primary" size="sm">
            {t("Workspace override")}
          </Badge>
        )}
      </td>
      <td className="px-3 py-2 text-secondary">v{template.version}</td>
      <td className="px-3 py-2 text-secondary">{new Date(template.updated_at).toLocaleString()}</td>
      <td className="px-3 py-2 text-right">
        <div className="flex items-center justify-end gap-2">
          <Link to={`/${slug}/prompts/${template.id}`} className="text-13 text-secondary hover:text-primary">
            {canEdit && !template.is_global_default ? t("Edit") : t("View")}
          </Link>
          {canEdit && onArchive && (
            <Button variant="tertiary-danger" size="sm" onClick={onArchive}>
              {t("Revert to default")}
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}

export default PromptsListPage;
