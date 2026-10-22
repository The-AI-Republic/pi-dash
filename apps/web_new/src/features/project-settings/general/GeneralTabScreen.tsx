// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings General tab (parity/specs/project-settings-general.md;
// PSET-005–009). One form reads the project row behind a skeleton, dims and
// disables itself for non-admins, validates per field, asks the identifier
// availability endpoint only when the short code changed, maps write-time
// conflicts onto their fields, and keeps repository binding and emblem
// upload on their own controls. Written from the spec; no old code reused.

import { Button, Skeleton } from "@pidash/kit";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import type { ProjectSettingsDetail } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useTimezones, filterTimezoneOptions, selectTimezoneOptions } from "../../../shared/pickers/index.js";
import { useProjectSettings } from "../components/ProjectSettingsLayout.js";
import { EmblemPicker } from "./components/EmblemPicker.js";
import { EmblemUploadDialog } from "./components/EmblemUploadDialog.js";
import { ExecutionSection } from "./components/ExecutionSection.js";
import { GeneralDetailsFields } from "./components/GeneralDetailsFields.js";
import { GeneralDangerZone } from "./components/GeneralDangerZone.js";
import { GeneralIdentity } from "./components/GeneralIdentity.js";
import { RepositoryRow } from "./components/RepositoryRow.js";
import {
  EMBLEM_UPLOAD_FAILED_MESSAGE,
  EMBLEM_UPLOAD_FAILED_TITLE,
  PROJECT_SAVE_FAILED_TITLE,
  useBindRepository,
  useSaveProjectGeneral,
  useUploadEmblem,
} from "./api/mutations.js";
import {
  identifierQueryOptions,
  seedProjectRow,
  useGeneralDetail,
  useIdentifierCheck,
  useRepositoryStatus,
} from "./api/queries.js";
import {
  buildGeneralPayload,
  generalConflictError,
  generalFormFrom,
  IDENTIFIER_CHECK_FAILED_MESSAGE,
  identifierUnavailable,
  reseatGeneralForm,
  saveFailureMessage,
  shapeIdentifier,
  uploadedEmblem,
  validateGeneralFields,
  type GeneralFieldErrors,
  type GeneralFormValues,
} from "./rules.js";

/** Save control copy at rest and while the write is in flight. */
export const SAVE_LABEL = "Update project";
export const SAVING_LABEL = "Updating";

export interface GeneralTabProps {
  workspaceSlug: string;
  projectId: string;
}

/** The form part that only exists once the stored row resolves. */
function GeneralTabBody({
  workspaceSlug,
  projectId,
  stored,
  canEdit,
}: GeneralTabProps & { stored: ProjectSettingsDetail; canEdit: boolean }): React.ReactElement {
  const [values, setValues] = React.useState<GeneralFormValues>(() => generalFormFrom(stored));
  const queryClient = useQueryClient();
  const [errors, setErrors] = React.useState<GeneralFieldErrors>({});
  const [cadenceDirty, setCadenceDirty] = React.useState(false);
  // True while submit waits on a fresh availability answer.
  const [checkingIdentifier, setCheckingIdentifier] = React.useState(false);
  const [emblemOpen, setEmblemOpen] = React.useState(false);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  // A refused emblem upload blocks every later save until a new upload
  // lands: the spec keeps the emblem and the accompanying fields off the
  // server (PSET-007) rather than saving a half-applied form.
  const [emblemUploadFailed, setEmblemUploadFailed] = React.useState(false);
  const save = useSaveProjectGeneral(workspaceSlug, projectId);
  const bind = useBindRepository(workspaceSlug, projectId);
  const upload = useUploadEmblem(workspaceSlug, projectId);
  const status = useRepositoryStatus(workspaceSlug, projectId, true);

  // A stored row arriving after the first render (or replaced by a save)
  // wins over the draft; a save in flight never resets the form. Comparing
  // the whole row, not just `updated_at`, keeps a write whose answer carries
  // the same timestamp from being dropped.
  const lastStored = React.useRef(stored);
  React.useEffect(() => {
    if (lastStored.current === stored || save.isPending) return;
    lastStored.current = stored;
    setValues((current) => reseatGeneralForm(stored, current));
  }, [stored, save.isPending]);

  const identifierChanged = values.identifier.toUpperCase() !== stored.identifier.toUpperCase();
  const precheck = useIdentifierCheck(
    workspaceSlug,
    identifierChanged && values.identifier.trim() !== "" ? values.identifier.trim() : null
  );

  const update = React.useCallback((patch: Partial<GeneralFormValues>): void => {
    const next = patch.identifier === undefined ? patch : { ...patch, identifier: shapeIdentifier(patch.identifier) };
    if (next.cadence !== undefined) setCadenceDirty(true);
    setValues((current) => ({ ...current, ...next }));
    setErrors((current) => {
      const remaining = { ...current };
      for (const field of Object.keys(next) as Array<keyof GeneralFormValues>) {
        if (field in remaining) delete remaining[field as keyof GeneralFieldErrors];
      }
      return remaining;
    });
  }, []);

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!canEdit || save.isPending || checkingIdentifier) return;
    if (emblemUploadFailed) {
      notifyError(EMBLEM_UPLOAD_FAILED_TITLE, EMBLEM_UPLOAD_FAILED_MESSAGE);
      return;
    }
    const fieldErrors = validateGeneralFields(values);
    setErrors(fieldErrors);
    if (Object.keys(fieldErrors).length > 0) return;

    void confirmThenSave(fieldErrors);
  };

  /**
   * A changed short code is checked again at submit time, whatever the
   * live hint says: a fetch still in flight or a failed one must not let
   * the write through. Only a completed "free" answer saves (PSET-006).
   */
  const confirmThenSave = async (fieldErrors: GeneralFieldErrors): Promise<void> => {
    const identifier = values.identifier.trim();
    if (identifierChanged && identifier !== "") {
      setCheckingIdentifier(true);
      let check: { exists: number };
      try {
        check = await queryClient.fetchQuery(identifierQueryOptions(getAppClient(), workspaceSlug, identifier));
      } catch {
        setCheckingIdentifier(false);
        setErrors({ ...fieldErrors, identifier: IDENTIFIER_CHECK_FAILED_MESSAGE });
        return;
      }
      setCheckingIdentifier(false);
      const unavailable = identifierUnavailable(check);
      if (unavailable !== null) {
        setErrors({ ...fieldErrors, identifier: unavailable });
        return;
      }
    }
    await save.mutateAsync(buildGeneralPayload({ values, cadenceDirty })).then(
      (saved) => {
        lastStored.current = saved;
        setValues((current) => reseatGeneralForm(saved, current));
        if (cadenceDirty) setCadenceDirty(true);
      },
      (error: unknown) => {
        const conflict = generalConflictError(error);
        if (conflict !== null) {
          setErrors((current) => ({ ...current, [conflict.field]: conflict.message }));
          return;
        }
        notifyError(PROJECT_SAVE_FAILED_TITLE, saveFailureMessage(error));
      }
    );
  };

  const timezones = useTimezones();
  const timezoneChoices = React.useMemo(
    () =>
      filterTimezoneOptions(selectTimezoneOptions(timezones.data?.timezones ?? []), "").map((option) => ({
        id: option.value,
        label: option.label,
        searchText: option.query,
      })),
    [timezones.data]
  );

  const formClass = canEdit ? "flex flex-col gap-(--space-8)" : "flex flex-col gap-(--space-8) opacity-60";
  return (
    <form
      data-testid="project-settings-general"
      aria-label="General settings"
      data-slice="NEWFRONT-572"
      className={formClass}
      onSubmit={submit}
    >
      <GeneralIdentity
        name={values.name}
        identifier={values.identifier}
        logoProps={values.logoProps}
        network={values.network}
        createdAt={stored.created_at}
        canEdit={canEdit}
        apiBaseUrl={getAppClient().baseUrl}
        onOpenMark={() => setEmblemOpen(true)}
        onOpenUpload={() => setUploadOpen(true)}
      />
      <GeneralDetailsFields
        values={values}
        errors={errors}
        disabled={!canEdit}
        checkingIdentifier={precheck.isFetching || checkingIdentifier}
        timezones={timezoneChoices}
        timezonesLoading={timezones.isPending}
        timezonesFailed={timezones.isError}
        onChange={update}
      />
      <RepositoryRow
        value={values.repoUrl}
        disabled={!canEdit}
        status={status.data}
        statusLoading={status.isPending}
        binding={bind.isPending}
        onChange={(repoUrl) => update({ repoUrl })}
        onBind={(repoUrl) => bind.mutate({ repoUrl })}
      />
      <ExecutionSection
        values={values}
        errors={errors}
        disabled={!canEdit}
        executorOptions={stored.agent_executor_options}
        onChange={update}
      />
      {canEdit ? (
        <GeneralDangerZone workspaceSlug={workspaceSlug} projectId={projectId} projectName={stored.name} />
      ) : null}
      {canEdit ? (
        <div className="flex justify-end">
          <Button type="submit" loading={save.isPending}>
            {save.isPending ? SAVING_LABEL : SAVE_LABEL}
          </Button>
        </div>
      ) : null}
      {canEdit ? (
        <EmblemPicker
          open={emblemOpen}
          onOpenChange={setEmblemOpen}
          logoProps={values.logoProps}
          onPick={(logoProps) => update({ logoProps })}
        />
      ) : null}
      {canEdit ? (
        <EmblemUploadDialog
          open={uploadOpen}
          onOpenChange={setUploadOpen}
          onUpload={(file) =>
            upload.mutateAsync({ file }).then(
              (url) => {
                setEmblemUploadFailed(false);
                return url;
              },
              (error: unknown) => {
                setEmblemUploadFailed(true);
                throw error;
              }
            )
          }
          onUploaded={(url) => update({ logoProps: uploadedEmblem(url) })}
        />
      ) : null}
    </form>
  );
}

/** Skeleton shaped like the identity row, fields and save control. */
export function GeneralSkeleton(): React.ReactElement {
  return (
    <div data-testid="project-settings-general-skeleton" className="flex flex-col gap-(--space-8)">
      <div className="flex items-center gap-(--space-4)">
        <Skeleton className="size-13 rounded-(--radius-control)" />
        <div className="flex flex-col gap-(--space-2)">
          <Skeleton className="h-6 w-48" />
          <Skeleton className="h-4 w-32" />
        </div>
      </div>
      <div className="grid grid-cols-1 gap-(--space-6) md:grid-cols-2">
        <Skeleton className="h-(--control-height-large) w-full" />
        <Skeleton className="h-(--control-height-large) w-full" />
        <Skeleton className="h-(--control-height-large) w-full" />
        <Skeleton className="h-(--control-height-large) w-full" />
      </div>
      <Skeleton className="h-(--control-height) w-40 self-end" />
    </div>
  );
}

/**
 * The tab. The shell resolves the project row first; this screen waits for
 * its own settings read (skeleton) and seeds the shell's row from it so a
 * direct visit paints the header.
 */
export function ProjectSettingsGeneralTab({ workspaceSlug, projectId }: GeneralTabProps): React.ReactElement {
  const { role } = useProjectSettings();
  const queryClient = useQueryClient();
  const detail = useGeneralDetail(workspaceSlug, projectId);
  const seeded = React.useRef(false);
  React.useEffect(() => {
    if (detail.data === undefined || seeded.current) return;
    seeded.current = true;
    seedProjectRow(queryClient, workspaceSlug, projectId, detail.data);
  }, [detail.data, projectId, queryClient, workspaceSlug]);

  if (detail.data === undefined) return <GeneralSkeleton />;
  return (
    <GeneralTabBody workspaceSlug={workspaceSlug} projectId={projectId} stored={detail.data} canEdit={role === 20} />
  );
}
