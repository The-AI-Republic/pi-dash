/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { AlertModalCore } from "@pi-dash/ui";
import type { AccountSwitch } from "@/services/desktop-account-sync";
import { registerAccountSwitchPrompt } from "@/services/desktop-account-sync";

/**
 * Asked by account sync before it switches this computer's `pidash` CLI —
 * and so its daemon — to the app's account while the CLI's own account has
 * runs in progress. Switching stops them. Declining keeps the CLI's account:
 * the app signs out and takes that account over instead.
 */
export function DesktopAccountSwitchDialog() {
  const { t } = useTranslation();
  const [request, setRequest] = useState<AccountSwitch | null>(null);
  const pending = useRef<((confirmed: boolean) => void) | null>(null);

  const settle = (confirmed: boolean) => {
    pending.current?.(confirmed);
    pending.current = null;
    setRequest(null);
  };

  useEffect(() => {
    registerAccountSwitchPrompt((next, resolve) => {
      pending.current?.(false);
      pending.current = resolve;
      setRequest(next);
    });
    return () => {
      registerAccountSwitchPrompt(null);
      pending.current?.(false);
      pending.current = null;
    };
  }, []);

  return (
    <AlertModalCore
      isOpen={request !== null}
      variant="primary"
      title={t("Switch Pi Dash on this computer?")}
      content={
        request && (
          <p className="mt-1">
            {t(
              "Pi Dash on this computer is signed in as {daemonEmail} with {count} runs in progress. Switch it to {desktopEmail}? Those runs will be stopped.",
              { daemonEmail: request.daemonEmail, desktopEmail: request.desktopEmail, count: request.activeRuns }
            )}
          </p>
        )
      }
      primaryButtonText={{ default: t("Switch"), loading: t("Switch") }}
      isSubmitting={false}
      handleClose={() => settle(false)}
      handleSubmit={() => settle(true)}
    />
  );
}
