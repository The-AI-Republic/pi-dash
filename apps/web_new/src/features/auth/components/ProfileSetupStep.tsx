// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile-setup onboarding step (AUTH-031; first on desktop). The required
// length-checked display name, an optional avatar through an upload dialog
// with change and remove actions, a password pair behind the AUTH-010
// gates for accounts that never set one, and marketing consent where
// editions ask for it. A failed save toasts but still advances. Written
// from parity/specs/auth.md.

import { Avatar, Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { useInstanceConfig, useMe } from "../../../core/session/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useSetPassword, useUpdateMe, useUploadUserAvatar } from "../api.js";
import { useUpdateProfile } from "../profile.js";
import { avatarFileError, validateProfileName } from "../onboarding.js";
import { canSubmitNewPassword, passwordMismatchError, weakSecretHint } from "../recovery.js";

export const PROFILE_SAVE_FAILED_TITLE = "Could not save your profile";
export const PROFILE_SAVE_FAILED_MESSAGE = "Check your connection and try again.";
export const AVATAR_UPLOAD_FAILED_MESSAGE = "Could not upload that image. Try a different file.";

export interface ProfileSetupStepProps {
  onAdvance: () => void;
}

export function ProfileSetupStep({ onAdvance }: ProfileSetupStepProps): React.ReactElement {
  const { data: me } = useMe();
  const { data: instanceConfig } = useInstanceConfig();
  const updateMe = useUpdateMe();
  const updateProfile = useUpdateProfile();
  const setPassword = useSetPassword();
  const uploadAvatar = useUploadUserAvatar();

  // The password pair is offered only to accounts that never set one.
  // (The cloud slice gates this further per CLOUD-020: cloud identities
  // never set a password here.)
  const offerPassword = me?.is_password_autoset === true;
  const offerConsent = instanceConfig?.is_self_managed !== true;

  const [name, setName] = React.useState(me?.first_name ?? "");
  const [nameBlurred, setNameBlurred] = React.useState(false);
  const [avatarUrl, setAvatarUrl] = React.useState<string | null>(me?.avatar_url ?? null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [dialogFile, setDialogFile] = React.useState<File | null>(null);
  const [dialogError, setDialogError] = React.useState<string | null>(null);
  const [password, setPasswordValue] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [confirmBlurred, setConfirmBlurred] = React.useState(false);
  const [showPassword, setShowPassword] = React.useState(false);
  const [showConfirm, setShowConfirm] = React.useState(false);
  const [consent, setConsent] = React.useState(true);
  const [saving, setSaving] = React.useState(false);

  const nameError = validateProfileName(name);
  const mismatch = offerPassword ? passwordMismatchError(password, confirm, confirmBlurred) : null;
  const weakHint = offerPassword ? weakSecretHint(password) : null;
  const passwordOk = !offerPassword || canSubmitNewPassword(password, confirm);
  const busy = saving || uploadAvatar.isPending;
  const canSubmit = nameError === null && passwordOk && !busy;

  const initialAvatarUrl = me?.avatar_url ?? null;

  const chooseFile = (file: File | null): void => {
    if (file === null) {
      setDialogFile(null);
      return;
    }
    const invalid = avatarFileError(file);
    if (invalid !== null) {
      setDialogFile(null);
      setDialogError(invalid);
      return;
    }
    setDialogError(null);
    setDialogFile(file);
  };

  const upload = async (): Promise<void> => {
    // Defensive: the Upload action stays disabled without a file, so this
    // only fires when a file is set.
    if (dialogFile === null) return;
    try {
      const url = await uploadAvatar.mutateAsync(dialogFile);
      setAvatarUrl(url);
      setDialogFile(null);
      setDialogError(null);
      setDialogOpen(false);
    } catch {
      setDialogError(AVATAR_UPLOAD_FAILED_MESSAGE);
    }
  };

  const removeAvatar = (): void => {
    setAvatarUrl(null);
    setDialogFile(null);
    setDialogError(null);
    setDialogOpen(false);
  };

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    setSaving(true);
    try {
      await Promise.all([
        updateMe.mutateAsync({
          first_name: name,
          ...(avatarUrl !== initialAvatarUrl ? { avatar: avatarUrl ?? "" } : {}),
        }),
        ...(offerPassword ? [setPassword.mutateAsync(password)] : []),
      ]);
    } catch {
      notifyError(PROFILE_SAVE_FAILED_TITLE, PROFILE_SAVE_FAILED_MESSAGE);
    } finally {
      setSaving(false);
    }
    if (offerConsent) {
      // Consent rides along: a failure must not block onboarding.
      updateProfile.mutate({ has_marketing_email_consent: consent });
    }
    onAdvance();
  };

  return (
    <section aria-labelledby="profile-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="profile-heading" className="text-h2 text-(--text)">
          Create your profile
        </h1>
        {/* Parity pin (AUTH-043): the scenario reads "Create your profile." */}
        <p className="text-body text-(--text-muted)">
          Create your profile. Tell everyone who you are. You can change this later.
        </p>
      </div>

      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-(--space-4)">
        <div className="flex items-center gap-(--space-3)">
          <Avatar name={name || me?.email || "You"} {...(avatarUrl ? { src: avatarUrl } : {})} size="large" />
          <Button variant="secondary" size="small" type="button" onClick={() => setDialogOpen(true)}>
            {avatarUrl ? "Change photo" : "Add a photo"}
          </Button>
        </div>

        <Input
          label="Your name"
          value={name}
          required
          autoFocus
          autoComplete="name"
          placeholder="Ada Byron"
          onChange={(event) => setName(event.target.value)}
          onBlur={() => setNameBlurred(true)}
          {...(nameBlurred && nameError !== null ? { error: nameError } : {})}
        />

        {offerPassword ? (
          <>
            <Input
              label="New password"
              type={showPassword ? "text" : "password"}
              autoComplete="new-password"
              required
              value={password}
              onChange={(event) => setPasswordValue(event.target.value)}
              trailing={
                <Button
                  variant="ghost"
                  size="small"
                  type="button"
                  onClick={() => setShowPassword((current) => !current)}
                  aria-label={showPassword ? "Hide new password" : "Show new password"}
                >
                  {showPassword ? "Hide" : "Show"}
                </Button>
              }
            />
            {weakHint ? <p className="text-body text-(--text-muted)">{weakHint}</p> : null}
            <Input
              label="Confirm password"
              type={showConfirm ? "text" : "password"}
              autoComplete="new-password"
              required
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
              onBlur={() => setConfirmBlurred(true)}
              {...(mismatch === null ? {} : { error: mismatch })}
              trailing={
                <Button
                  variant="ghost"
                  size="small"
                  type="button"
                  onClick={() => setShowConfirm((current) => !current)}
                  aria-label={showConfirm ? "Hide confirmation" : "Show confirmation"}
                >
                  {showConfirm ? "Hide" : "Show"}
                </Button>
              }
            />
          </>
        ) : null}

        <Button type="submit" variant="primary" size="large" loading={saving} disabled={!canSubmit}>
          Continue
        </Button>

        {offerConsent ? (
          <label className="text-body flex cursor-pointer items-center gap-(--space-2) text-(--text-muted)">
            <input type="checkbox" checked={consent} onChange={(event) => setConsent(event.target.checked)} />
            Email me product updates and tips
          </label>
        ) : null}
      </form>

      <Dialog
        title={avatarUrl ? "Change photo" : "Add a photo"}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        nonDismissable={uploadAvatar.isPending}
        actions={
          <>
            {avatarUrl ? (
              <Button variant="ghost" onClick={removeAvatar} disabled={uploadAvatar.isPending}>
                Remove
              </Button>
            ) : null}
            <Button
              variant="primary"
              onClick={() => void upload()}
              loading={uploadAvatar.isPending}
              disabled={dialogFile === null}
            >
              Upload
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-(--space-3)">
          <label className="text-body flex cursor-pointer flex-col gap-(--space-2) text-(--text)">
            Choose an image
            <input
              type="file"
              accept="image/jpeg,image/png,image/webp"
              onChange={(event) => chooseFile(event.target.files?.[0] ?? null)}
            />
          </label>
          {dialogFile ? (
            <p className="text-body text-(--text-muted)">
              {dialogFile.name} ({dialogFile.size} bytes)
            </p>
          ) : null}
          {dialogError ? <p className="text-body text-(--danger)">{dialogError}</p> : null}
        </div>
      </Dialog>
    </section>
  );
}
