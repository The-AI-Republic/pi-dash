/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { range } from "lodash-es";
import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// icons
import { Eye, EyeOff, RefreshCw } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { CopyIcon } from "@pi-dash/propel/icons";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Tooltip } from "@pi-dash/propel/tooltip";
import type { IWebhook } from "@pi-dash/types";
// ui
import { csvDownload, copyTextToClipboard } from "@pi-dash/utils";
// hooks
import { useWebhook } from "@/hooks/store/use-webhook";
import { useWorkspace } from "@/hooks/store/use-workspace";
// types
import { usePlatformOS } from "@/hooks/use-platform-os";
// utils
import { getCurrentHookAsCSV } from "../utils";
// hooks

type Props = {
  data: Partial<IWebhook>;
};

export const WebhookSecretKey = observer(function WebhookSecretKey(props: Props) {
  const { data } = props;
  // states
  const [isRegenerating, setIsRegenerating] = useState(false);
  const [shouldShowKey, setShouldShowKey] = useState(false);
  // router
  const { workspaceSlug, webhookId } = useParams();
  // store hooks
  const { currentWorkspace } = useWorkspace();
  const { currentWebhook, regenerateSecretKey, webhookSecretKey } = useWebhook();
  const { isMobile } = usePlatformOS();
  const { t } = useTranslation();
  const handleCopySecretKey = () => {
    if (!webhookSecretKey) return;

    copyTextToClipboard(webhookSecretKey)
      .then(() =>
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: `${t("Success")}`,
          message: t("Secret key copied to clipboard."),
        })
      )
      .catch(() =>
        setToast({
          type: TOAST_TYPE.ERROR,
          title: `${t("Error")}!`,
          message: t("Error occurred while copying secret key."),
        })
      );
  };

  const handleRegenerateSecretKey = () => {
    if (!workspaceSlug || !data.id) return;

    setIsRegenerating(true);

    regenerateSecretKey(workspaceSlug.toString(), data.id)
      .then(() => {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: `${t("Success")}`,
          message: "New key regenerated successfully.",
        });

        if (currentWebhook && webhookSecretKey) {
          const csvData = getCurrentHookAsCSV(currentWorkspace, currentWebhook, webhookSecretKey);
          csvDownload(csvData, `webhook-secret-key-${Date.now()}`);
        }
      })
      .catch((err) =>
        setToast({
          type: TOAST_TYPE.ERROR,
          title: `${t("Error")}!`,
          message: err?.error ?? t("Something went wrong. Please try again."),
        })
      )
      .finally(() => setIsRegenerating(false));
  };

  const toggleShowKey = () => setShouldShowKey((prevState) => !prevState);

  const SECRET_KEY_OPTIONS = [
    { label: "View secret key", Icon: shouldShowKey ? EyeOff : Eye, onClick: toggleShowKey, key: "eye" },
    { label: "Copy secret key", Icon: CopyIcon, onClick: handleCopySecretKey, key: "copy" },
  ];

  return (
    <>
      {(data || webhookSecretKey) && (
        <div className="space-y-2">
          {webhookId && (
            <div className="text-13 font-medium">{t("Secret key")}</div>
          )}
          <div className="text-11 text-placeholder">{t("Generate a token to sign-in to the webhook payload")}</div>
          <div className="flex flex-col gap-4 md:flex-row md:items-center">
            <div className="flex h-8 max-w-lg flex-grow items-center justify-between self-stretch rounded-sm border border-subtle px-2">
              <div className="overflow-hidden font-medium select-none">
                {shouldShowKey ? (
                  <p className="text-11">{webhookSecretKey}</p>
                ) : (
                  <div className="mr-2 flex items-center gap-1.5 overflow-hidden">
                    {range(30).map((index) => (
                      <div key={index} className="h-1 w-1 flex-shrink-0 rounded-full bg-(--text-color-disabled)" />
                    ))}
                  </div>
                )}
              </div>
              {webhookSecretKey && (
                <div className="flex items-center gap-2">
                  {SECRET_KEY_OPTIONS.map((option) => (
                    <Tooltip key={option.key} tooltipContent={option.label} isMobile={isMobile}>
                      <button type="button" className="grid flex-shrink-0 place-items-center" onClick={option.onClick}>
                        <option.Icon className="h-3 w-3 text-placeholder" />
                      </button>
                    </Tooltip>
                  ))}
                </div>
              )}
            </div>
            {data && (
              <div>
                <Button
                  onClick={handleRegenerateSecretKey}
                  variant="secondary"
                  size="lg"
                  loading={isRegenerating}
                  prependIcon={<RefreshCw />}
                >
                  {isRegenerating ? `${t("Re-generating")}...` : t("Re-generate key")}
                </Button>
              </div>
            )}
          </div>
        </div>
      )}
    </>
  );
});
