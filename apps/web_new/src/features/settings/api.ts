// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings mutations (parity/specs/profile.md §5/§6). The reads reuse the
// session queries (current user, instance flags, workspace list); this
// module owns the account writes, their cache refreshes and their
// feedback. Every write invalidates the current-user row so the identity
// header, sidebar and user menu all follow.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  changeEmail,
  changePassword,
  checkEmail,
  deleteLegacyUserFileAsset,
  deleteUserAsset,
  generateEmailChangeCode,
  updateMe,
  uploadUserAvatar,
  type MeUpdate,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/toaster.js";
import type { AvatarRemoval } from "./lib/avatar.js";
import { describeEmailSendFailure, emailVerifyMessage } from "./lib/email.js";
import { describePasswordFailure } from "./lib/password.js";

export const PROFILE_SAVED_TITLE = "Success!";
export const PROFILE_SAVED_MESSAGE = "Profile updated successfully.";
export const PROFILE_SAVE_FAILED_TITLE = "Could not update your profile";
export const PROFILE_SAVE_FAILED_MESSAGE = "Something went wrong. Please try again.";

export const AVATAR_REMOVED_TITLE = "Success!";
export const AVATAR_REMOVED_MESSAGE = "Profile picture deleted successfully.";
export const AVATAR_REMOVE_FAILED_TITLE = "Could not delete your profile picture";
export const AVATAR_REMOVE_FAILED_MESSAGE = "There was an error deleting your profile picture. Please try again.";
export const AVATAR_UPLOAD_FAILED_TITLE = "Could not upload that image";
export const AVATAR_UPLOAD_FAILED_MESSAGE = "Try a different file.";

/** The name, display-name and avatar write one save posts. */
export interface ProfileSaveInput {
  first_name: string;
  last_name: string;
  display_name: string;
  /** Avatar reference; sent only when it changed. */
  avatar?: string;
}

/**
 * A failed save's feedback. The server's first field error, when it names
 * one, is the useful line; every other failure keeps the generic line.
 */
export function profileSaveFailureMessage(error: unknown): string {
  const record = (error ?? {}) as { message?: unknown; fields?: unknown };
  const fields = record.fields;
  if (fields !== null && typeof fields === "object") {
    for (const value of Object.values(fields as Record<string, unknown>)) {
      if (Array.isArray(value) && typeof value[0] === "string" && value[0].length > 0) return value[0];
      if (typeof value === "string" && value.length > 0) return value;
    }
  }
  if (typeof record.message === "string" && record.message.length > 0) return record.message;
  return PROFILE_SAVE_FAILED_MESSAGE;
}

/** PATCH /api/users/me/ with the names (and the avatar when it changed). */
export function useSaveProfile() {
  const queryClient = useQueryClient();
  return useMutation<MeUpdate, Error, ProfileSaveInput>({
    mutationFn: (input) => updateMe(getAppClient(), input),
    onSuccess: (updated) => {
      notifySuccess(PROFILE_SAVED_TITLE, PROFILE_SAVED_MESSAGE);
      // The PATCH answer lacks the computed avatar URL, so merge the
      // written fields and let the refreshed row win.
      queryClient.setQueryData(sessionKeys.me, (previous: Record<string, unknown> | undefined) =>
        previous === undefined ? previous : { ...previous, ...updated }
      );
    },
    onError: (error) => {
      notifyError(PROFILE_SAVE_FAILED_TITLE, profileSaveFailureMessage(error));
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

/** One avatar image upload; resolves to the asset URL to store on the row. */
export function useUploadAvatar() {
  const queryClient = useQueryClient();
  return useMutation<string, Error, File>({
    mutationFn: (file) => uploadUserAvatar(getAppClient(), file),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

/** Store the returned asset URL on the user row. */
export function useStoreAvatar() {
  const queryClient = useQueryClient();
  return useMutation<MeUpdate, Error, string>({
    mutationFn: (avatar) => updateMe(getAppClient(), { avatar }),
    onSuccess: () => {
      notifySuccess(PROFILE_SAVED_TITLE, PROFILE_SAVED_MESSAGE);
    },
    onError: (error) => {
      notifyError(PROFILE_SAVE_FAILED_TITLE, profileSaveFailureMessage(error));
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

/** Delete the stored asset (v2 or legacy path) and clear it from the row. */
export function useRemoveAvatar() {
  const queryClient = useQueryClient();
  return useMutation<MeUpdate, Error, AvatarRemoval>({
    mutationFn: async (removal) => {
      const client = getAppClient();
      if (removal === null) return updateMe(client, { avatar: "" });
      if (removal.kind === "asset") await deleteUserAsset(client, removal.assetId);
      else await deleteLegacyUserFileAsset(client, removal.assetKey);
      return updateMe(client, { avatar: "" });
    },
    onSuccess: () => {
      notifySuccess(AVATAR_REMOVED_TITLE, AVATAR_REMOVED_MESSAGE);
    },
    onError: () => {
      notifyError(AVATAR_REMOVE_FAILED_TITLE, AVATAR_REMOVE_FAILED_MESSAGE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

/** Shape check plus taken-address refusal for the email flow's first step. */
export function useCheckEmailTaken() {
  return useMutation<boolean, Error, string>({
    mutationFn: async (email) => (await checkEmail(getAppClient(), email)).existing,
  });
}

/** Ask the server to mail the one-time code to the new address. */
export function useGenerateEmailCode() {
  return useMutation<unknown, Error, string>({
    mutationFn: (email) => generateEmailChangeCode(getAppClient(), email),
  });
}

/** Verify the code, move the account, and report failures inline. */
export function useConfirmEmailChange() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, { email: string; code: string }>({
    mutationFn: (input) => changeEmail(getAppClient(), input.email, input.code),
    // A wrong code leaves the account untouched; success ends the session
    // server-side, so the caller signs out and clears the cache.
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}

export interface ChangePasswordInput {
  oldPassword?: string;
  newPassword: string;
}

/**
 * POST /auth/change-password/ with the old secret where the account has
 * one. Success toasts and resets the form; failures toast and, for a weak
 * guess or a missing secret, also report on a field.
 */
export function useChangePassword() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, ChangePasswordInput>({
    mutationFn: (input) => changePassword(getAppClient(), input),
    onSuccess: () => {
      notifySuccess(PROFILE_SAVED_TITLE, "Password changed successfully.");
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
    onError: (error) => {
      const failure = describePasswordFailure(error);
      notifyError(failure.title, failure.message);
    },
  });
}

export { describeEmailSendFailure, emailVerifyMessage };
