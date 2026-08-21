/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTranslation } from "@pi-dash/i18n";
import { AlertModalCore } from "@pi-dash/ui";

type Props = {
  isOpen: boolean;
  isSubmitting: boolean;
  oldIdentifier: string;
  newIdentifier: string;
  onClose: () => void;
  onConfirm: () => void;
};

export function ProjectIdentifierChangeAlert(props: Props) {
  const { isOpen, isSubmitting, oldIdentifier, newIdentifier, onClose, onConfirm } = props;
  const { t } = useTranslation();

  return (
    <AlertModalCore
      isOpen={isOpen}
      handleClose={onClose}
      handleSubmit={onConfirm}
      isSubmitting={isSubmitting}
      title={t("Change project ID?")}
      content={
        <div className="space-y-2">
          <p>
            {t(
              "Changing the project ID from {oldIdentifier} to {newIdentifier} can disconnect local runners configured with the current ID.",
              { oldIdentifier, newIdentifier }
            )}
          </p>
          <p>
            {t(
              "After this change, update each affected runner's project_slug to {newIdentifier} and restart the Pi Dash runner daemon. Agent runs may remain queued until the runners reconnect.",
              { newIdentifier }
            )}
          </p>
        </div>
      }
      primaryButtonText={{
        default: t("Change project ID"),
        loading: t("Changing project ID"),
      }}
    />
  );
}
