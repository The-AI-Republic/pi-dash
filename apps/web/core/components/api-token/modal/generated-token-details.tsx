/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { CopyIcon } from "@pi-dash/propel/icons";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Tooltip } from "@pi-dash/propel/tooltip";
import type { IApiToken } from "@pi-dash/types";
// ui
import { renderFormattedDate, renderFormattedTime, copyTextToClipboard } from "@pi-dash/utils";
// helpers
// types
import { usePlatformOS } from "@/hooks/use-platform-os";
// hooks

type Props = {
  handleClose: () => void;
  tokenDetails: IApiToken;
};

export function GeneratedTokenDetails(props: Props) {
  const { handleClose, tokenDetails } = props;
  const { isMobile } = usePlatformOS();
  const { t } = useTranslation();
  const copyApiToken = (token: string) => {
    copyTextToClipboard(token).then(() =>
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: `${t("Success")}!`,
        message: t("Token copied to clipboard."),
      })
    );
  };

  return (
    <div className="w-full p-5">
      <div className="w-full space-y-3 text-wrap">
        <h3 className="text-16 leading-6 font-medium text-primary">{t("Key created")}</h3>
        <p className="text-13 text-placeholder">{t("Copy and save this secret key in Pi Dash Pages. You can't see this key after you hit Close. A CSV file containing the key has been downloaded.")}</p>
      </div>
      <button
        type="button"
        onClick={() => copyApiToken(tokenDetails.token ?? "")}
        className="mt-4 flex w-full items-center justify-between truncate rounded-md border-[0.5px] border-subtle px-3 py-2 text-13 font-medium outline-none"
      >
        <span className="truncate pr-2">{tokenDetails.token}</span>
        <Tooltip tooltipContent="Copy secret key" isMobile={isMobile}>
          <CopyIcon className="h-4 w-4 flex-shrink-0 text-placeholder" />
        </Tooltip>
      </button>
      <div className="mt-6 flex items-center justify-between">
        <p className="text-11 text-placeholder">
          {tokenDetails.expired_at
            ? `Expires ${renderFormattedDate(tokenDetails.expired_at)} at ${renderFormattedTime(tokenDetails.expired_at)}`
            : "Never expires"}
        </p>
        <Button variant="secondary" onClick={handleClose}>
          {t("Close")}
        </Button>
      </div>
    </div>
  );
}
