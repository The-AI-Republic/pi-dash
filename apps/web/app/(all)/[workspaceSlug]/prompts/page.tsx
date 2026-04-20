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
        title: t("prompts.toast.created_title"),
        message: t("prompts.toast.created_message"),
      });
      navigate(`/${slug}/prompts/${created.id}`);
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("prompts.toast.error_title"),
        message: err?.error ?? t("prompts.toast.customize_failed"),
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
        title: t("prompts.toast.reverted_title"),
        message: t("prompts.toast.reverted_message"),
      });
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("prompts.toast.error_title"),
        message: err?.error ?? t("prompts.toast.revert_failed"),
      });
    } finally {
      setArchiving(false);
    }
  }

  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} · ${t("prompts.title")}` : t("prompts.title");

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHead title={pageTitle} />

      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-16 font-semibold text-primary">{t("prompts.title")}</h1>
          <p className="mt-1 text-13 text-secondary">{t("prompts.subtitle")}</p>
        </div>
        {canEdit && !workspaceOverride && (
          <Button onClick={handleCustomize} loading={customizing} disabled={customizing}>
            {t("prompts.customize")}
          </Button>
        )}
      </header>

      <section className="rounded-md border border-subtle">
        <table className="w-full text-13">
          <thead className="bg-layer-1 text-left text-secondary">
            <tr>
              <th className="px-3 py-2">{t("prompts.columns.name")}</th>
              <th className="px-3 py-2">{t("prompts.columns.scope")}</th>
              <th className="px-3 py-2">{t("prompts.columns.version")}</th>
              <th className="px-3 py-2">{t("prompts.columns.updated")}</th>
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
                  {t("prompts.list.empty")}
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
        title={t("prompts.revert.confirm_title")}
        content={t("prompts.revert.confirm_body")}
        primaryButtonText={{
          default: t("prompts.revert.confirm"),
          loading: t("prompts.revert.confirm"),
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
            {t("prompts.scope.default")}
          </Badge>
        ) : (
          <Badge variant="accent-primary" size="sm">
            {t("prompts.scope.workspace")}
          </Badge>
        )}
      </td>
      <td className="px-3 py-2 text-secondary">v{template.version}</td>
      <td className="px-3 py-2 text-secondary">{new Date(template.updated_at).toLocaleString()}</td>
      <td className="px-3 py-2 text-right">
        <div className="flex items-center justify-end gap-2">
          <Link to={`/${slug}/prompts/${template.id}`} className="text-13 text-secondary hover:text-primary">
            {canEdit && !template.is_global_default ? t("prompts.actions.edit") : t("prompts.actions.view")}
          </Link>
          {canEdit && onArchive && (
            <Button variant="tertiary-danger" size="sm" onClick={onArchive}>
              {t("prompts.actions.revert")}
            </Button>
          )}
        </div>
      </td>
    </tr>
  );
}

export default PromptsListPage;
