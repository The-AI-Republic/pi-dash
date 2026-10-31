// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reusable workspace creation form (AUTH-034, AUTH-035). The name, slug,
// and team-size fields with their checks: the slug derives from the name
// but stays editable, malformed values block submit inline, and submit
// rejects reserved and taken slugs before anything is created. The
// onboarding workspace step mounts this with pill sizes; the standalone
// route (S9, AUTH-040) mounts the same unit with full-page chrome and a
// dropdown size picker through renderSizeSelector. Written from
// parity/specs/auth.md.

import { Button, Input } from "@pidash/kit";
import type { CreatedWorkspace } from "@pidash/api-client";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useCheckWorkspaceSlug, useCreateWorkspace } from "../api.js";
import {
  deriveWorkspaceSlug,
  isReservedWorkspaceSlug,
  SLUG_TAKEN_MESSAGE,
  validateWorkspaceName,
  validateWorkspaceSlug,
  WORKSPACE_TEAM_SIZES,
} from "../workspace.js";

export const CREATE_SUCCESS_TITLE = "Workspace created";
export const CREATE_FAILED_TITLE = "Could not create the workspace";
export const TEAM_SIZE_REQUIRED_MESSAGE = "Choose your team size.";

/** The team-size control the form renders (pills by default, dropdown for S9). */
export interface TeamSizeSelectorField {
  value: string;
  options: readonly string[];
  onSelect: (size: string) => void;
  disabled: boolean;
  /** Id of the visible "Team size" label, for the control to reference. */
  labelId: string;
}

export interface WorkspaceCreateFormProps {
  /** Called with the created workspace and the chosen size after a successful create. */
  onCreated: (workspace: CreatedWorkspace, size: string) => void;
  /** Custom size control; defaults to the team-size pills. */
  renderSizeSelector?: (field: TeamSizeSelectorField) => React.ReactElement;
}

function TeamSizePills(field: TeamSizeSelectorField): React.ReactElement {
  return (
    <div role="radiogroup" aria-labelledby={field.labelId} className="flex flex-wrap gap-(--space-2)">
      {field.options.map((option) => (
        <Button
          key={option}
          type="button"
          variant={field.value === option ? "primary" : "secondary"}
          aria-pressed={field.value === option}
          onClick={() => field.onSelect(option)}
          disabled={field.disabled}
        >
          {option}
        </Button>
      ))}
    </div>
  );
}

export function WorkspaceCreateForm({ onCreated, renderSizeSelector }: WorkspaceCreateFormProps): React.ReactElement {
  const slugCheck = useCheckWorkspaceSlug();
  const createWorkspace = useCreateWorkspace();

  const [name, setName] = React.useState("");
  const [slug, setSlug] = React.useState("");
  const [slugCustomized, setSlugCustomized] = React.useState(false);
  const [size, setSize] = React.useState("");
  const [nameDirty, setNameDirty] = React.useState(false);
  const [slugDirty, setSlugDirty] = React.useState(false);
  const [submitAttempted, setSubmitAttempted] = React.useState(false);
  const [slugTaken, setSlugTaken] = React.useState(false);

  const busy = slugCheck.isPending || createWorkspace.isPending;
  const nameError = validateWorkspaceName(name);
  const slugError = validateWorkspaceSlug(slug);
  const showNameError = (nameDirty || submitAttempted) && nameError !== null;
  const showSlugError = (slugDirty || submitAttempted) && slugError !== null;
  const showSizeError = submitAttempted && size === "";
  const submittable = nameError === null && slugError === null && size !== "";
  // A malformed slug blocks submit before anything is created; every other
  // gap is revealed by submitting.
  const malformedSlug = slug.trim() !== "" && slugError !== null;
  const shownFormatError = showSlugError ? slugError : null;
  const slugFieldError = slugTaken ? SLUG_TAKEN_MESSAGE : shownFormatError;

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    setSubmitAttempted(true);
    if (!submittable) return;
    if (isReservedWorkspaceSlug(slug)) {
      setSlugTaken(true);
      return;
    }
    try {
      const free = await slugCheck.mutateAsync(slug);
      if (!free) {
        setSlugTaken(true);
        return;
      }
      const created = await createWorkspace.mutateAsync({ name: name.trim(), slug, organization_size: size });
      notifySuccess(CREATE_SUCCESS_TITLE);
      onCreated(created, size);
    } catch (error) {
      notifyError(CREATE_FAILED_TITLE, (error as Error).message);
    }
  };

  const selector =
    renderSizeSelector ?? ((field: TeamSizeSelectorField): React.ReactElement => <TeamSizePills {...field} />);

  return (
    <section aria-labelledby="create-workspace-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="create-workspace-heading" className="text-h2 text-(--text)">
          Create your workspace
        </h1>
        <p className="text-body text-(--text-muted)">Bring your work together in one place.</p>
      </div>

      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-(--space-4)">
        <Input
          label="Workspace name"
          value={name}
          autoFocus
          onChange={(event) => {
            const next = event.target.value;
            setName(next);
            setNameDirty(true);
            if (!slugCustomized) setSlug(deriveWorkspaceSlug(next));
          }}
          {...(showNameError && nameError ? { error: nameError } : {})}
        />

        <Input
          label="Workspace URL"
          hint="Lowercase letters, numbers, hyphens, and underscores."
          value={slug}
          onChange={(event) => {
            setSlugCustomized(true);
            setSlugDirty(true);
            setSlug(event.target.value.toLowerCase());
            setSlugTaken(false);
          }}
          {...(slugFieldError ? { error: slugFieldError } : {})}
        />

        <div className="flex flex-col gap-(--space-2)">
          <span id="create-workspace-size-label" className="text-body font-medium text-(--text)">
            Team size
          </span>
          {selector({
            value: size,
            options: WORKSPACE_TEAM_SIZES,
            onSelect: setSize,
            disabled: busy,
            labelId: "create-workspace-size-label",
          })}
          {showSizeError ? <p className="text-body text-(--danger)">{TEAM_SIZE_REQUIRED_MESSAGE}</p> : null}
        </div>

        <Button type="submit" variant="primary" size="large" loading={busy} disabled={busy || malformedSlug}>
          Create workspace
        </Button>
      </form>
    </section>
  );
}
