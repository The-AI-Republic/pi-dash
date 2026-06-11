/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { useTranslation } from "@pi-dash/i18n";

export function AuthHero({ className = "" }: { className?: string }) {
  const { t } = useTranslation();
  return (
    <div className={`flex w-full max-w-md flex-col gap-4 text-center lg:text-left ${className}`}>
      <h1 className="text-3xl sm:text-4xl leading-tight font-semibold text-primary">
        {t("Pi Dash: Your AI Employee Management Platform")}
      </h1>
      <p className="text-lg text-tertiary">{t("Pi Dash helps you utilize best out of Claude Fable 5")}</p>
    </div>
  );
}
