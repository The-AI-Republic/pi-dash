// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab (parity/specs/profile.md §6, PROF-011/012/013/014). Edits the
// member's own names with per-field validation, a locked account address,
// the avatar dialog above the form, an optional two-step email change
// where the instance has mail configured, and the danger zone on
// self-managed instances. Every write is own-record only.

import { Avatar, Button, Input } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useInstanceConfig, useMe } from "../../../core/session/index.js";
import { resolveFileUrl } from "../../../shared/shell/index.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { useDeactivateAccount } from "../../auth/index.js";
import {
  useRemoveAvatar,
  useSaveProfile,
  useStoreAvatar,
  useUploadAvatar,
  AVATAR_UPLOAD_FAILED_MESSAGE,
  AVATAR_UPLOAD_FAILED_TITLE,
} from "../api.js";
import { avatarRemovalFor } from "../lib/avatar.js";
import { NAME_MESSAGES, namesAreValid, validateNames, type NameField } from "../lib/names.js";
import { AvatarUploadDialog } from "./AvatarUploadDialog.js";
import { ChangeEmailDialog } from "./ChangeEmailDialog.js";
import { DeactivateAccountSection } from "./DeactivateAccountSection.js";

export const GENERAL_SAVE_LABEL = "Save changes";
export const GENERAL_SAVING_LABEL = "Saving";

export function GeneralSettingsTab(): React.ReactElement {
  const { data: me } = useMe();
  const { data: instance } = useInstanceConfig();
  const save = useSaveProfile();
  const upload = useUploadAvatar();
  const storeAvatar = useStoreAvatar();
  const removeAvatar = useRemoveAvatar();
  const deactivate = useDeactivateAccount();

  const [values, setValues] = React.useState({ first_name: "", last_name: "", display_name: "" });
  const [submitted, setSubmitted] = React.useState(false);
  const [avatarOpen, setAvatarOpen] = React.useState(false);
  const [emailOpen, setEmailOpen] = React.useState(false);
  const [avatarError, setAvatarError] = React.useState<string | null>(null);

  // The form seeds from the loaded row once and follows later writes.
  const seededFor = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (me === undefined) return;
    if (seededFor.current === me.id) return;
    seededFor.current = me.id;
    setValues({
      first_name: me.first_name,
      last_name: me.last_name,
      display_name: me.display_name,
    });
  }, [me]);

  if (me === undefined) {
    return (
      <div data-testid="general-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
        <div className="h-16 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        <div className="h-10 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        <div className="h-10 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
      </div>
    );
  }

  const avatarSrc = resolveFileUrl(me.avatar_url ?? me.avatar, getAppClient().baseUrl);
  const errors = submitted ? validateNames(values) : {};
  const busy = save.isPending || upload.isPending || storeAvatar.isPending || removeAvatar.isPending;
  const mailConfigured = instance?.is_smtp_configured === true;
  const selfManaged = instance?.is_self_managed === true;
  const fullName = [values.first_name, values.last_name].filter((part) => part !== "").join(" ");

  const setField = (field: NameField, value: string): void => {
    setValues((previous) => ({ ...previous, [field]: value }));
  };

  const fieldError = (field: NameField): string | undefined => errors[field];

  const submit = (event: React.FormEvent): void => {
    event.preventDefault();
    setSubmitted(true);
    if (!namesAreValid(values)) return;
    save.mutate({
      first_name: values.first_name,
      last_name: values.last_name,
      display_name: values.display_name,
    });
  };

  const uploadFile = (file: File): void => {
    setAvatarError(null);
    upload.mutate(file, {
      onSuccess: (url) => {
        storeAvatar.mutate(url, {
          onSuccess: () => setAvatarOpen(false),
          onError: () => setAvatarError(AVATAR_UPLOAD_FAILED_MESSAGE),
        });
      },
      onError: () => {
        notifyError(AVATAR_UPLOAD_FAILED_TITLE, AVATAR_UPLOAD_FAILED_MESSAGE);
      },
    });
  };

  const removePhoto = (): void => {
    setAvatarError(null);
    removeAvatar.mutate(avatarRemovalFor(me.avatar), {
      onSuccess: () => setAvatarOpen(false),
    });
  };

  return (
    <div className="flex flex-col gap-(--space-8)">
      <header
        data-testid="profile-settings-identity"
        className="flex items-center gap-(--space-4) border-b border-(--border) pb-(--space-6)"
      >
        <button
          type="button"
          aria-label="Change profile photo"
          className="rounded-(--radius-full) focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--accent)"
          onClick={() => setAvatarOpen(true)}
        >
          <Avatar name={fullName || me.email} src={avatarSrc ?? ""} size="large" />
        </button>
        <div className="flex min-w-0 flex-col">
          <span className="text-emphasis font-medium text-(--text)">{fullName || me.display_name}</span>
          <span className="text-meta truncate text-(--text-muted)">{me.email}</span>
        </div>
      </header>

      <form data-testid="general-settings-form" noValidate className="flex flex-col gap-(--space-6)" onSubmit={submit}>
        <div className="grid gap-(--space-4) sm:grid-cols-2">
          <Input
            label="First name"
            id="first_name"
            name="first_name"
            maxLength={50}
            autoComplete="given-name"
            placeholder="Enter your first name"
            value={values.first_name}
            {...({ error: fieldError("first_name") } as { error?: string })}
            onChange={(event) => setField("first_name", event.target.value)}
          />
          <Input
            label="Last name"
            id="last_name"
            name="last_name"
            maxLength={50}
            autoComplete="family-name"
            placeholder="Enter your last name"
            hint="Optional"
            value={values.last_name}
            {...({ error: fieldError("last_name") } as { error?: string })}
            onChange={(event) => setField("last_name", event.target.value)}
          />
          <Input
            label="Display name"
            id="display_name"
            name="display_name"
            maxLength={50}
            placeholder="Enter your display name"
            required
            value={values.display_name}
            {...({ error: fieldError("display_name") } as { error?: string })}
            onChange={(event) => setField("display_name", event.target.value)}
          />
          <div className="flex flex-col gap-(--space-2)">
            <Input
              label="Email"
              id="email"
              name="email"
              type="email"
              autoComplete="email"
              value={me.email}
              disabled
              readOnly
            />
            {mailConfigured ? (
              <Button variant="ghost" size="small" className="w-fit self-start" onClick={() => setEmailOpen(true)}>
                Change email
              </Button>
            ) : null}
          </div>
        </div>

        <div>
          <Button type="submit" variant="primary" loading={save.isPending} disabled={busy}>
            {save.isPending ? GENERAL_SAVING_LABEL : GENERAL_SAVE_LABEL}
          </Button>
        </div>
      </form>

      <AvatarUploadDialog
        open={avatarOpen}
        onOpenChange={(next) => {
          if (!next) setAvatarError(null);
          setAvatarOpen(next);
        }}
        currentSrc={avatarSrc}
        uploading={upload.isPending || storeAvatar.isPending}
        removing={removeAvatar.isPending}
        onUpload={uploadFile}
        onRemove={removePhoto}
      />
      {/* The server-side upload error rides the toast above; this keeps the
          dialog's refusal line separate from pick-time validation. */}
      {avatarError !== null ? (
        <p role="alert" className="text-meta text-(--danger)">
          {avatarError}
        </p>
      ) : null}

      {mailConfigured ? (
        <ChangeEmailDialog
          open={emailOpen}
          onOpenChange={setEmailOpen}
          currentEmail={me.email}
          onChanged={handleSignedOut}
        />
      ) : null}

      {selfManaged ? (
        <DeactivateAccountSection
          confirming={deactivate.isPending}
          onConfirm={() => {
            deactivate.mutate(undefined, {
              onSuccess: (landing) => {
                handleSignedOut(landing);
              },
            });
          }}
        />
      ) : null}
    </div>
  );
}

/** Leave the settings page after an action that ends the session. */
function handleSignedOut(landing?: string): void {
  if (typeof window === "undefined") return;
  window.location.assign(landing ?? "/");
}

export { NAME_MESSAGES };
