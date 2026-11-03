// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-project dialog (SHELL-040, SHELL-041, SHELL-042; written
// from parity/specs/shell.md §11). One shared component for every
// creation entry: the list header, the empty state, the quickstart
// card and (once NEWFRONT-293 lands) the palette. Identity,
// visibility, lead and icon with an auto-derived short code;
// duplicates map onto their fields. The form offers no cover
// control and the submit is a single POST with no cover value.
import { Button, Combobox, Dialog, Input, Menu, Popover, Textarea, cn } from "@pidash/kit";
import type { WorkspaceMember } from "@pidash/api-client";
import { Check, ChevronDown, Globe, Lock } from "lucide-react";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useCreateProject } from "../api/mutations.js";
import {
  buildCreatePayload,
  CREATE_ICON_CHOICES,
  CREATE_IDENTIFIER_MAX_LENGTH,
  CREATE_DESCRIPTION_PLACEHOLDER,
  CREATE_REPO_URL_MAX_LENGTH,
  CREATE_NETWORK_PRIVATE,
  CREATE_NETWORK_PUBLIC,
  deriveProjectIdentifier,
  pickCreateIcon,
  resolveBrowserTimezone,
  sanitizeProjectIdentifier,
  selectCreateFieldErrors,
  validateCreateFields,
  type CreateFieldErrors,
} from "../lib/project-create.js";

const CREATE_FAILED_TITLE = "Couldn't create the project";

export interface CreateProjectDialogProps {
  workspaceSlug: string;
  /** Member directory for the lead picker; undefined while resolving. */
  members: WorkspaceMember[] | undefined;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function CreateProjectDialog({
  workspaceSlug,
  members,
  open,
  onOpenChange,
}: CreateProjectDialogProps): React.ReactElement {
  const [name, setName] = React.useState("");
  const [identifier, setIdentifier] = React.useState("");
  const [autoSync, setAutoSync] = React.useState(true);
  const [description, setDescription] = React.useState("");
  const [repoUrl, setRepoUrl] = React.useState("");
  const [baseBranch, setBaseBranch] = React.useState("");
  const [template, setTemplate] = React.useState("blank");
  const [network, setNetwork] = React.useState<number>(CREATE_NETWORK_PUBLIC);
  const [lead, setLead] = React.useState<string | null>(null);
  const [icon, setIcon] = React.useState(() => pickCreateIcon());
  const [timezone, setTimezone] = React.useState(() => resolveBrowserTimezone());
  const [fieldErrors, setFieldErrors] = React.useState<CreateFieldErrors>({});
  const create = useCreateProject();

  // Fresh form per visit: reopening re-prefills the icon and timezone.
  React.useEffect(() => {
    if (!open) return;
    setName("");
    setIdentifier("");
    setAutoSync(true);
    setDescription("");
    setRepoUrl("");
    setBaseBranch("");
    setTemplate("blank");
    setNetwork(CREATE_NETWORK_PUBLIC);
    setLead(null);
    setIcon(pickCreateIcon());
    setTimezone(resolveBrowserTimezone());
    setFieldErrors({});
  }, [open]);

  const leadName = members?.find((row) => row.member.id === lead)?.member.display_name ?? "Lead";
  const isPublic = network !== CREATE_NETWORK_PRIVATE;

  function onNameChange(value: string): void {
    setName(value);
    if (autoSync) setIdentifier(deriveProjectIdentifier(value));
    setFieldErrors((prev) => ({ ...prev, name: undefined }));
  }

  function onIdentifierChange(value: string): void {
    setIdentifier(sanitizeProjectIdentifier(value).slice(0, CREATE_IDENTIFIER_MAX_LENGTH));
    setAutoSync(false);
    setFieldErrors((prev) => ({ ...prev, identifier: undefined }));
  }

  function submit(): void {
    const local = validateCreateFields({ name, identifier, repoUrl, baseBranch });
    if (Object.keys(local).length > 0) {
      setFieldErrors(local);
      return;
    }
    create.mutate(
      {
        workspaceSlug,
        payload: buildCreatePayload({
          name,
          identifier,
          description,
          repoUrl,
          baseBranch,
          network,
          projectLead: lead,
          icon,
          timezone,
        }),
      },
      {
        onSuccess: () => {
          onOpenChange(false);
        },
        onError: (error) => {
          const mapped = selectCreateFieldErrors(error);
          if (mapped.name === undefined && mapped.identifier === undefined) {
            notifyError(CREATE_FAILED_TITLE, (error as Error).message);
            return;
          }
          setFieldErrors(mapped);
        },
      }
    );
  }

  return (
    <Dialog
      title="New project"
      description="Set up a space for the team's work."
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={create.isPending} onClick={submit}>
            {create.isPending ? "Creating…" : "Create project"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Combobox
          trigger={
            <Button variant="secondary" size="small" aria-label="Project template" className="w-fit">
              Blank project
            </Button>
          }
          options={[{ id: "blank", label: "Blank project" }]}
          value={template}
          onSelect={setTemplate}
          listLabel="Project template"
          searchEnabled={false}
          emptyText="No templates"
        />
        <div className="flex items-start gap-(--space-3)">
          <Popover
            trigger={
              <button
                type="button"
                aria-label="Project icon"
                className="text-h3 grid h-10 w-10 shrink-0 cursor-pointer place-items-center rounded-(--radius-control) bg-(--subtle)"
              >
                {icon}
              </button>
            }
            title="Project icon"
          >
            <div role="group" aria-label="Pick an icon" className="grid max-w-56 grid-cols-8 gap-(--space-1)">
              {CREATE_ICON_CHOICES.map((choice) => (
                <button
                  key={choice}
                  type="button"
                  aria-label={`Use ${choice} as the project icon`}
                  aria-pressed={choice === icon}
                  onClick={() => setIcon(choice)}
                  className={cn(
                    "text-lg grid h-8 w-8 cursor-pointer place-items-center rounded-(--radius-control)",
                    choice === icon ? "bg-(--subtle)" : null
                  )}
                >
                  {choice}
                </button>
              ))}
            </div>
          </Popover>
          <div className="grid min-w-0 flex-1 grid-cols-1 gap-(--space-3) md:grid-cols-3">
            <div className="md:col-span-2">
              <Input
                label="Name"
                value={name}
                onChange={(event) => onNameChange(event.target.value)}
                placeholder="Project name"
                autoComplete="off"
                {...(fieldErrors.name === undefined ? {} : { error: fieldErrors.name })}
              />
            </div>
            <Input
              label="Short code"
              value={identifier}
              onChange={(event) => onIdentifierChange(event.target.value)}
              placeholder="Project ID"
              autoComplete="off"
              className="uppercase"
              {...(fieldErrors.identifier === undefined ? {} : { error: fieldErrors.identifier })}
            />
          </div>
        </div>
        <Textarea
          label="Description"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          placeholder={CREATE_DESCRIPTION_PLACEHOLDER}
          rows={3}
        />
        <Input
          label="Repository URL"
          value={repoUrl}
          maxLength={CREATE_REPO_URL_MAX_LENGTH}
          onChange={(event) => {
            setRepoUrl(event.target.value);
            setFieldErrors((prev) => ({ ...prev, repoUrl: undefined }));
          }}
          placeholder="Repository URL, e.g. https://github.com/team/project"
          autoComplete="off"
          {...(fieldErrors.repoUrl === undefined ? {} : { error: fieldErrors.repoUrl })}
        />
        <Input
          label="Base branch"
          value={baseBranch}
          maxLength={128}
          onChange={(event) => {
            setBaseBranch(event.target.value);
            setFieldErrors((prev) => ({ ...prev, baseBranch: undefined }));
          }}
          placeholder="Base branch (blank uses the remote default)"
          autoComplete="off"
          {...(fieldErrors.baseBranch === undefined ? {} : { error: fieldErrors.baseBranch })}
        />
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <Menu
            trigger={
              <Button variant="secondary" size="small" aria-label="Visibility">
                {isPublic ? (
                  <Globe size={14} strokeWidth={2} aria-hidden="true" />
                ) : (
                  <Lock size={14} strokeWidth={2} aria-hidden="true" />
                )}
                {isPublic ? "Public" : "Private"}
                <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
              </Button>
            }
            items={[
              {
                label: "Public",
                icon:
                  network === CREATE_NETWORK_PUBLIC ? (
                    <Check size={14} strokeWidth={2} aria-hidden="true" />
                  ) : undefined,
                onSelect: () => setNetwork(CREATE_NETWORK_PUBLIC),
              },
              {
                label: "Private",
                icon:
                  network === CREATE_NETWORK_PRIVATE ? (
                    <Check size={14} strokeWidth={2} aria-hidden="true" />
                  ) : undefined,
                onSelect: () => setNetwork(CREATE_NETWORK_PRIVATE),
              },
            ]}
          />
          {members === undefined ? (
            <Button variant="secondary" size="small" disabled aria-label="Project lead">
              Lead
              <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
            </Button>
          ) : (
            <Menu
              trigger={
                <Button variant="secondary" size="small" aria-label="Project lead">
                  {leadName}
                  <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
                </Button>
              }
              items={[
                {
                  label: "No lead",
                  icon: lead === null ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : undefined,
                  onSelect: () => setLead(null),
                },
                ...members.map((row) => ({
                  label: row.member.display_name,
                  itemKey: row.member.id,
                  icon: lead === row.member.id ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : undefined,
                  onSelect: () => setLead(row.member.id),
                })),
              ]}
            />
          )}
        </div>
        <p className="text-caption text-(--text-muted)">Timezone: {timezone}</p>
      </div>
    </Dialog>
  );
}
