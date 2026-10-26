/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { AlertModalCore, Checkbox } from "@pi-dash/ui";
import type { SignOutChoice } from "@/services/agent-runtime";
import { registerSignOutPrompt } from "@/services/agent-runtime";

/**
 * The desktop's sign-out confirmation.
 *
 * Signing out signs this computer's `pidash` CLI out as well, which stops the
 * runs its daemon has in progress; when there are any, the dialog says so.
 *
 * Signing out removes credentials and, by default, nothing else: an account's
 * chats with the built-in agent stay on this computer, hidden until that
 * account signs in again. The checkbox is the one place a user can ask for
 * them to be deleted instead, so it starts unticked every time the dialog
 * opens.
 */
export function DesktopSignOutDialog() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [deleteChatHistory, setDeleteChatHistory] = useState(false);
  const [activeRuns, setActiveRuns] = useState(0);
  const pending = useRef<((choice: SignOutChoice | null) => void) | null>(null);

  const settle = (choice: SignOutChoice | null) => {
    pending.current?.(choice);
    pending.current = null;
    setIsOpen(false);
  };

  useEffect(() => {
    registerSignOutPrompt((resolve, context) => {
      // A second request while one is open replaces it; the first is cancelled
      // rather than left waiting forever.
      pending.current?.(null);
      pending.current = resolve;
      setDeleteChatHistory(false);
      setActiveRuns(context.activeRuns);
      setIsOpen(true);
    });
    return () => {
      registerSignOutPrompt(null);
      pending.current?.(null);
      pending.current = null;
    };
  }, []);

  return (
    <AlertModalCore
      isOpen={isOpen}
      variant="primary"
      title={t("Are you sure you want to sign out?")}
      content={
        <>
          {activeRuns > 0 && (
            <p role="alert" className="mt-1 text-danger-primary">
              {activeRuns === 1
                ? t("1 run in progress on this computer will be stopped.")
                : t("{count} runs in progress on this computer will be stopped.", { count: activeRuns })}
            </p>
          )}
          <label className="mt-2 flex cursor-pointer items-center gap-2 text-primary">
            <Checkbox checked={deleteChatHistory} onChange={(event) => setDeleteChatHistory(event.target.checked)} />
            {t("Delete chat history")}
          </label>
          <p className="mt-1">
            {t("Removes this account's chats with the built-in agent from this computer. Task folders are kept.")}
          </p>
        </>
      }
      primaryButtonText={{ default: t("Sign out"), loading: t("Sign out") }}
      isSubmitting={false}
      handleClose={() => settle(null)}
      handleSubmit={() => settle({ deleteChatHistory })}
    />
  );
}
