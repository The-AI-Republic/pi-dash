/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { AlertModalCore, Checkbox } from "@pi-dash/ui";
import type { SignOutChoice } from "@/services/agent-runtime";
import { canSignOutComputer, registerSignOutPrompt } from "@/services/agent-runtime";

/**
 * The desktop's sign-out confirmation.
 *
 * Signing out of the app signs out only the app (design §4.6): Pi Dash on this
 * computer — the daemon, the built-in engine and every runner — stays signed
 * in, because it does not depend on the app. Two opt-ins, both unticked every
 * time the dialog opens:
 *
 * - "Delete chat history" removes this account's local chats;
 * - "Also sign out Pi Dash on this computer" runs `pidash auth logout`. Shown
 *   only when the computer's daemon is signed in as this account.
 */
export function DesktopSignOutDialog() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [deleteChatHistory, setDeleteChatHistory] = useState(false);
  const [signOutComputer, setSignOutComputer] = useState(false);
  const [offerComputer, setOfferComputer] = useState(false);
  const pending = useRef<((choice: SignOutChoice | null) => void) | null>(null);

  const settle = (choice: SignOutChoice | null) => {
    pending.current?.(choice);
    pending.current = null;
    setIsOpen(false);
  };

  useEffect(() => {
    registerSignOutPrompt((resolve) => {
      // A second request while one is open replaces it; the first is cancelled
      // rather than left waiting forever.
      pending.current?.(null);
      pending.current = resolve;
      setDeleteChatHistory(false);
      setSignOutComputer(false);
      setOfferComputer(canSignOutComputer());
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
          <p className="mt-1">{t("Pi Dash on this computer, its built-in engine and its runners stay signed in.")}</p>
          <label className="mt-2 flex cursor-pointer items-center gap-2 text-primary">
            <Checkbox checked={deleteChatHistory} onChange={(event) => setDeleteChatHistory(event.target.checked)} />
            {t("Delete chat history")}
          </label>
          <p className="mt-1">
            {t("Removes this account's chats with the built-in agent from this computer. Task folders are kept.")}
          </p>
          {offerComputer && (
            <>
              <label className="mt-2 flex cursor-pointer items-center gap-2 text-primary">
                <Checkbox checked={signOutComputer} onChange={(event) => setSignOutComputer(event.target.checked)} />
                {t("Also sign out Pi Dash on this computer")}
              </label>
              <p className="mt-1">
                {t("Runs `pidash auth logout`: the runners on this computer stop taking work until you sign in again.")}
              </p>
            </>
          )}
        </>
      }
      primaryButtonText={{ default: t("Sign out"), loading: t("Sign out") }}
      isSubmitting={false}
      handleClose={() => settle(null)}
      handleSubmit={() => settle({ deleteChatHistory, signOutComputer: offerComputer && signOutComputer })}
    />
  );
}
