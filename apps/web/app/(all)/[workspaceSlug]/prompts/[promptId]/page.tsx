/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { Link, useParams } from "react-router";
import useSWR from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IPromptTemplate } from "@pi-dash/types";
import { Badge, Button, Input } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { usePromptTemplate } from "@/hooks/store/use-prompt-template";
import { useUserPermissions } from "@/hooks/store/user";

/**
 * Prompt template detail + editor. Read-only for members / guests; admins
 * get an editable textarea for workspace-scoped rows with a side-by-side
 * preview pane that calls the server preview endpoint against an issue
 * id they paste in.
 *
 * The global default is always read-only here — platform operators edit
 * it out-of-band.
 */
const PromptDetailPage = observer(function PromptDetailPage() {
  const { workspaceSlug, promptId } = useParams<{
    workspaceSlug: string;
    promptId: string;
  }>();
  const { allowPermissions } = useUserPermissions();
  const { t } = useTranslation();
  const promptStore = usePromptTemplate();

  const slug = workspaceSlug ?? "";
  const id = promptId ?? "";

  const { data: templates } = useSWR<IPromptTemplate[]>(slug ? ["prompt-templates", slug] : null, () =>
    promptStore.fetchTemplates(slug)
  );

  const record = promptStore.getTemplateById(id);

  const canEdit =
    allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, slug) &&
    record !== null &&
    !record.is_global_default;

  const [draft, setDraft] = useState<string>("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [issueId, setIssueId] = useState<string>("");
  const [preview, setPreview] = useState<string>("");
  const [previewing, setPreviewing] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);

  // Seed the draft once the record has loaded, and re-seed when we switch
  // to a different template id or the server's version counter advances.
  // Depending on the full record would re-run on every observable mutation;
  // keying on id+version keeps the draft stable while the user is typing.
  const recordId = record?.id;
  const recordVersion = record?.version;
  const recordBody = record?.body;
  useEffect(() => {
    if (recordBody !== undefined) {
      setDraft(recordBody);
      setDirty(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recordId, recordVersion]);

  if (!record) {
    return (
      <div className="p-6 text-13 text-secondary">
        {templates === undefined ? t("prompts.detail.loading") : t("prompts.detail.not_found")}
      </div>
    );
  }

  async function handleSave() {
    if (!canEdit || !dirty || saving) return;
    setSaving(true);
    try {
      await promptStore.updateTemplate(slug, id, { body: draft });
      setDirty(false);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("prompts.toast.saved_title"),
        message: t("prompts.toast.saved_message"),
      });
    } catch (e: unknown) {
      const err = e as { error?: string; body?: string[] } | null;
      const message = err?.body?.[0] ?? err?.error ?? t("prompts.toast.save_failed");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("prompts.toast.error_title"),
        message,
      });
    } finally {
      setSaving(false);
    }
  }

  async function handlePreview() {
    if (previewing) return;
    if (!issueId) {
      setPreviewError(t("prompts.preview.missing_issue_id"));
      return;
    }
    setPreviewing(true);
    setPreviewError(null);
    try {
      const resp = await promptStore.previewTemplate(slug, id, issueId, canEdit ? draft : undefined);
      setPreview(resp.prompt);
    } catch (e: unknown) {
      const err = e as { error?: string; detail?: string } | null;
      setPreviewError(err?.detail ?? err?.error ?? t("prompts.preview.failed"));
      setPreview("");
    } finally {
      setPreviewing(false);
    }
  }

  const pageTitle = record.is_global_default ? t("prompts.detail.default_title") : t("prompts.detail.workspace_title");

  return (
    <div className="flex flex-col gap-4 p-6">
      <PageHead title={pageTitle} />

      <header className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-16 font-semibold text-primary">{record.name}</h1>
            <Badge variant={record.is_global_default ? "accent-neutral" : "accent-primary"} size="sm">
              {record.is_global_default ? t("prompts.scope.default") : t("prompts.scope.workspace")}
            </Badge>
            <span className="text-13 text-secondary">v{record.version}</span>
          </div>
          <p className="mt-1 text-13 text-secondary">
            {record.is_global_default
              ? t("prompts.detail.default_description")
              : t("prompts.detail.workspace_description")}
          </p>
        </div>
        <Link to={`/${slug}/prompts`} className="text-13 text-secondary hover:text-primary">
          {t("prompts.detail.back")}
        </Link>
      </header>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <h2 className="text-13 font-medium text-primary">{t("prompts.detail.body")}</h2>
            {canEdit && (
              <div className="flex items-center gap-2">
                {dirty && <span className="text-11 text-secondary">{t("prompts.detail.unsaved")}</span>}
                <Button onClick={handleSave} loading={saving} disabled={!dirty || saving} size="sm">
                  {t("prompts.detail.save")}
                </Button>
              </div>
            )}
          </div>
          <textarea
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setDirty(e.target.value !== record.body);
            }}
            readOnly={!canEdit}
            spellCheck={false}
            className="bg-layer-base font-mono focus:border-primary min-h-[60vh] w-full resize-y rounded-md border border-subtle p-3 text-11 leading-5 text-primary focus:outline-none"
          />
        </section>

        <section className="flex flex-col gap-2">
          <h2 className="text-13 font-medium text-primary">{t("prompts.preview.title")}</h2>
          <div className="flex items-center gap-2">
            <Input
              value={issueId}
              onChange={(e) => setIssueId(e.target.value)}
              placeholder={t("prompts.preview.issue_id_placeholder")}
              className="font-mono flex-1 text-11"
            />
            <Button
              onClick={handlePreview}
              loading={previewing}
              disabled={previewing || !issueId}
              size="sm"
              variant="outline-primary"
            >
              {t("prompts.preview.run")}
            </Button>
          </div>
          {previewError && (
            <div className="border-destructive text-destructive rounded border bg-layer-1 p-2 text-11">
              {previewError}
            </div>
          )}
          <pre className="font-mono min-h-[60vh] w-full overflow-auto rounded-md border border-subtle bg-layer-1 p-3 text-11 leading-5 whitespace-pre-wrap text-primary">
            {preview || t("prompts.preview.empty")}
          </pre>
        </section>
      </div>
    </div>
  );
});

export default PromptDetailPage;
