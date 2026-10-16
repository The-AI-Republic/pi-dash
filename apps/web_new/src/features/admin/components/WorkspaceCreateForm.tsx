// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-workspace form (ADM-043–045, from parity/specs/admin.md
// §5.7–§5.9). Name, slug, and a required size-band select; the slug
// prefixes the web origin and re-derives from the name on every name
// keystroke while staying independently editable and forced lowercase.
// Each field reports its validation message once touched, and submit
// opens only when the whole form validates. Submit probes slug
// availability first: a taken or restricted slug raises the inline
// taken error with no create, while probe and create failures toast
// the error and stay. Success clears the draft and returns to the
// list; anything else keeps the typed values (they persist across
// unmount through the draft store).
//
// Parity-driver contract (for the NEWFRONT-521 gate author): the form
// carries data-testid="admin-workspace-create-form". Fields hang off
// associated labels ("Workspace name", "Workspace URL",
// "Organization size"); the name input has id
// "workspace-create-name", the slug input id "workspace-create-slug",
// and the select id "workspace-create-org-size", with each field's
// validation message in "#{id}-error" (role="alert"). The slug prefix
// ("<origin>/") is data-testid="admin-workspace-create-slug-prefix";
// the taken error is role="alert"
// data-testid="admin-workspace-create-slug-taken" and the charset hint
// role="alert" data-testid="admin-workspace-create-slug-invalid". The
// select's first option is the unpicked placeholder (value ""); bands
// carry value and label alike ("Just myself", "2-10", "11-50",
// "51-200", "201-500", "500+"). Submit is
// data-testid="admin-workspace-create-submit", labelled "Create
// workspace" at rest and "Creating workspace" while the probe plus
// create are in flight; toasts ride the dashboard ToastHost ("Success!"
// / "Workspace created successfully." on create, "Error!" on failure).
import { Button, FieldScaffold, Input } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { useCreateInstanceWorkspace } from "../hooks.js";
import { useWorkspaceCreateDraftStore } from "../stores.js";
import {
  buildWorkspaceCreateInput,
  deriveWorkspaceSlug,
  formatWorkspaceSlugPrefix,
  isWorkspaceCreateSubmittable,
  ORGANIZATION_SIZE_BANDS,
  shouldShowInvalidSlugHint,
  validateWorkspaceCreateName,
  validateWorkspaceCreateSlug,
  WORKSPACE_CREATE_BUSY_LABEL,
  WORKSPACE_CREATE_INVALID_SLUG_HINT,
  WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE,
  WORKSPACE_CREATE_SUBMIT_LABEL,
  WORKSPACE_CREATE_TAKEN_MESSAGE,
  WorkspaceSlugTakenError,
} from "../workspaceCreate.js";

/** Placeholder label for the unpicked size band. */
export const WORKSPACE_CREATE_SIZE_PLACEHOLDER = "Select size";

/** Where a successful create lands: the workspace list. */
export const WORKSPACE_CREATE_SUCCESS_PATH = "/god-mode/workspace";

export function WorkspaceCreateForm(): React.ReactElement {
  const name = useWorkspaceCreateDraftStore((state) => state.name);
  const slug = useWorkspaceCreateDraftStore((state) => state.slug);
  const organizationSize = useWorkspaceCreateDraftStore((state) => state.organizationSize);
  const creating = useCreateInstanceWorkspace();
  const navigate = useNavigate();
  const [nameTouched, setNameTouched] = React.useState(false);
  const [slugTouched, setSlugTouched] = React.useState(false);
  const [sizeTouched, setSizeTouched] = React.useState(false);
  const [taken, setTaken] = React.useState(false);

  const nameError = validateWorkspaceCreateName(name);
  const slugError = validateWorkspaceCreateSlug(slug);
  const submittable = isWorkspaceCreateSubmittable(name, slug, organizationSize);

  const onSubmit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!submittable || creating.isPending) return;
    creating.mutate(buildWorkspaceCreateInput(name, slug, organizationSize), {
      onSuccess: () => {
        setTaken(false);
        useWorkspaceCreateDraftStore.getState().resetDraft();
        void navigate({ to: WORKSPACE_CREATE_SUCCESS_PATH });
      },
      onError: (error) => {
        if (error instanceof WorkspaceSlugTakenError) setTaken(true);
      },
    });
  };

  return (
    <form data-testid="admin-workspace-create-form" onSubmit={onSubmit} className="flex flex-col gap-(--space-6)">
      <Input
        id="workspace-create-name"
        label="Workspace name"
        value={name}
        {...(nameTouched && nameError !== null ? { error: nameError } : {})}
        onChange={(event) => {
          const next = event.target.value;
          useWorkspaceCreateDraftStore.getState().setDraft({ name: next, slug: deriveWorkspaceSlug(next) });
          setNameTouched(true);
        }}
        data-testid="admin-workspace-create-name"
      />
      <FieldScaffold
        label="Workspace URL"
        {...(slugTouched && slugError !== null ? { error: slugError } : {})}
        fieldId="workspace-create-slug"
      >
        {(describedBy) => (
          <div className="text-body flex h-(--control-height) w-full items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) focus-within:border-(--accent)">
            <span
              data-testid="admin-workspace-create-slug-prefix"
              className="shrink-0 whitespace-nowrap text-(--text-muted)"
            >
              {formatWorkspaceSlugPrefix(window.location.origin)}
            </span>
            <input
              id="workspace-create-slug"
              aria-describedby={describedBy}
              value={slug}
              onChange={(event) => {
                useWorkspaceCreateDraftStore.getState().setDraft({ slug: event.target.value.toLowerCase() });
                setSlugTouched(true);
              }}
              data-testid="admin-workspace-create-slug"
              className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-(--text-muted)"
            />
          </div>
        )}
      </FieldScaffold>
      {taken ? (
        <p role="alert" data-testid="admin-workspace-create-slug-taken" className="text-meta text-(--danger)">
          {WORKSPACE_CREATE_TAKEN_MESSAGE}
        </p>
      ) : null}
      {slugTouched && shouldShowInvalidSlugHint(slug) ? (
        <p role="alert" data-testid="admin-workspace-create-slug-invalid" className="text-meta text-(--danger)">
          {WORKSPACE_CREATE_INVALID_SLUG_HINT}
        </p>
      ) : null}
      <FieldScaffold
        label="Organization size"
        {...(sizeTouched && organizationSize === "" ? { error: WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE } : {})}
        fieldId="workspace-create-org-size"
      >
        {(describedBy) => (
          <select
            id="workspace-create-org-size"
            aria-describedby={describedBy}
            value={organizationSize}
            onChange={(event) => {
              useWorkspaceCreateDraftStore.getState().setDraft({ organizationSize: event.target.value });
              setSizeTouched(true);
            }}
            data-testid="admin-workspace-create-org-size"
            className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          >
            <option value="">{WORKSPACE_CREATE_SIZE_PLACEHOLDER}</option>
            {ORGANIZATION_SIZE_BANDS.map((band) => (
              <option key={band} value={band}>
                {band}
              </option>
            ))}
          </select>
        )}
      </FieldScaffold>
      <div>
        <Button
          type="submit"
          variant="primary"
          disabled={!submittable || creating.isPending}
          loading={creating.isPending}
          data-testid="admin-workspace-create-submit"
        >
          {creating.isPending ? WORKSPACE_CREATE_BUSY_LABEL : WORKSPACE_CREATE_SUBMIT_LABEL}
        </Button>
      </div>
    </form>
  );
}
