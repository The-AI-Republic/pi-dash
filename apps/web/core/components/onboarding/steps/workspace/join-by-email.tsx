/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Spinner } from "@pi-dash/ui";
import { checkEmailValidity, cn } from "@pi-dash/utils";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useUserSettings } from "@/hooks/store/user";
// services
import { WorkspaceService } from "@/services/workspace.service";
// local components
import { CommonOnboardingHeader } from "../common";

type Props = {
  // Called after a request is recorded and the user should wait for approval.
  onRequestSent: (adminEmail: string) => void;
  // Called when the typed email resolves to a workspace the user already
  // belongs to — the backend routes them straight in instead of pending.
  onAlreadyMember: () => Promise<void> | void;
  // Return to the "create a workspace" view.
  onBack: () => void;
};

const workspaceService = new WorkspaceService();

export const WorkspaceJoinByEmailStep = observer(function WorkspaceJoinByEmailStep({
  onRequestSent,
  onAlreadyMember,
  onBack,
}: Props) {
  // states
  const [adminEmail, setAdminEmail] = useState("");
  const [emailError, setEmailError] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { fetchWorkspaces } = useWorkspace();
  const { fetchCurrentUserSettings } = useUserSettings();

  const isEmailValid = checkEmailValidity(adminEmail.trim());

  const handleSubmit = async () => {
    if (isSubmitting) return;
    const email = adminEmail.trim();
    if (!checkEmailValidity(email)) {
      setEmailError(true);
      return;
    }
    setEmailError(false);
    setIsSubmitting(true);
    try {
      const response = await workspaceService.createWorkspaceJoinRequest({ admin_email: email });
      // The email resolved only to a workspace the user is already a member of:
      // route them straight in rather than showing a pending screen.
      if (response?.workspace_slug) {
        await fetchWorkspaces();
        await fetchCurrentUserSettings();
        await onAlreadyMember();
        return;
      }
      // Neutral "request sent" — identical whether or not the email was a real
      // admin. Move the user to the pending-approval holding view.
      onRequestSent(email);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("We couldn't send your request. Please try again."),
      });
      setIsSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col gap-10">
      <CommonOnboardingHeader
        title="Join an existing workspace"
        description="Enter the email of a workspace admin to request access."
      />
      <div className="flex flex-col gap-2">
        <label
          className="text-13 font-medium text-tertiary after:ml-0.5 after:text-danger-primary after:content-['*']"
          htmlFor="admin_email"
        >
          {t("Workspace admin email")}
        </label>
        <input
          id="admin_email"
          name="admin_email"
          type="email"
          value={adminEmail}
          onChange={(event) => {
            setAdminEmail(event.target.value);
            if (emailError) setEmailError(false);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && isEmailValid && !isSubmitting) {
              event.preventDefault();
              void handleSubmit();
            }
          }}
          placeholder={t("admin@company.com")}
          className={cn(
            "w-full rounded-md border border-strong bg-surface-1 px-3 py-2 text-secondary transition-all duration-200 placeholder:text-placeholder focus:border-transparent focus:ring-2 focus:ring-accent-strong focus:outline-none",
            {
              "border-danger-strong": emailError,
            }
          )}
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
        />
        {emailError && <span className="text-13 text-danger-primary">{t("Please enter a valid email address.")}</span>}
      </div>
      <div className="flex flex-col gap-4">
        <Button
          variant="primary"
          size="xl"
          className="w-full"
          onClick={handleSubmit}
          disabled={!isEmailValid || isSubmitting}
        >
          {isSubmitting ? <Spinner height="20px" width="20px" /> : t("Send join request")}
        </Button>
        <Button variant="ghost" size="xl" className="w-full" onClick={onBack} disabled={isSubmitting}>
          {t("Create your own workspace instead")}
        </Button>
      </div>
    </div>
  );
});
