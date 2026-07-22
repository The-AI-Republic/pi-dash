/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { MailCheck } from "lucide-react";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
// local components
import { CommonOnboardingHeader } from "../common";

type Props = {
  // The admin email the request was addressed to, shown for reassurance.
  adminEmail: string;
  // Escape hatch: abandon waiting and create your own workspace instead.
  onCreateInstead: () => void;
};

export const WorkspacePendingApprovalStep = observer(function WorkspacePendingApprovalStep({
  adminEmail,
  onCreateInstead,
}: Props) {
  // pi dash hooks
  const { t } = useTranslation();

  return (
    <div className="flex flex-col gap-10">
      <CommonOnboardingHeader
        title="Waiting for approval"
        description="Your request to join is pending. You'll get access as soon as it's approved."
      />
      <div className="flex flex-col items-center gap-4 rounded-lg border border-subtle bg-surface-2 px-4 py-8 text-center">
        <span className="flex size-12 items-center justify-center rounded-full bg-surface-1">
          <MailCheck className="size-6 text-accent-primary" />
        </span>
        <div className="flex flex-col gap-1">
          <p className="text-13 font-medium text-secondary">{t("Request sent")}</p>
          {adminEmail ? (
            <p className="text-13 text-tertiary">
              {t("We've notified")} <span className="font-medium text-secondary">{adminEmail}</span>.{" "}
              {t("You'll join automatically once they approve.")}
            </p>
          ) : (
            <p className="text-13 text-tertiary">{t("You'll join automatically once your request is approved.")}</p>
          )}
        </div>
      </div>
      <div className="flex flex-col gap-4">
        <Button variant="ghost" size="xl" className="w-full" onClick={onCreateInstead}>
          {t("Create your own workspace instead")}
        </Button>
      </div>
    </div>
  );
});
