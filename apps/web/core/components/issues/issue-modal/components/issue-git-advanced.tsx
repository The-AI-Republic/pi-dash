/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import type { Control } from "react-hook-form";
import { Controller } from "react-hook-form";
import { ChevronRight } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import type { TIssue } from "@pi-dash/types";
import { Input } from "@pi-dash/ui";

type Props = {
  control: Control<TIssue>;
  handleFormChange: () => void;
};

/**
 * Collapsed-by-default advanced section on the issue form. Today only exposes
 * `git_work_branch`; callers should continue adding fields here rather than
 * cluttering the primary attributes strip.
 */
export function IssueGitAdvanced(props: Props) {
  const { control, handleFormChange } = props;
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <div className="px-5">
      <button
        type="button"
        className="flex items-center gap-1 text-11 font-medium text-secondary hover:text-primary"
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        <ChevronRight className={`h-3 w-3 transition-transform ${open ? "rotate-90" : ""}`} />
        {t("issue_advanced_git") || "Advanced — git"}
      </button>
      {open && (
        <div className="mt-2 flex flex-col gap-1">
          <label htmlFor="git_work_branch" className="text-11 text-secondary">
            {t("git_work_branch") || "Work branch"}
            <span className="pl-1 text-placeholder">
              {t("git_work_branch_hint") || "(optional — pin this issue to an existing remote branch)"}
            </span>
          </label>
          <Controller
            name="git_work_branch"
            control={control}
            rules={{
              maxLength: {
                value: 128,
                message: t("git_work_branch_too_long") || "Branch name is too long",
              },
              pattern: {
                value: /^[A-Za-z0-9._/-]*$/,
                message: t("git_work_branch_invalid_chars") || "Only letters, numbers, and . _ / - are allowed",
              },
            }}
            render={({ field: { value, onChange }, fieldState: { error } }) => (
              <>
                <Input
                  id="git_work_branch"
                  name="git_work_branch"
                  type="text"
                  value={value ?? ""}
                  onChange={(e) => {
                    onChange(e);
                    handleFormChange();
                  }}
                  hasError={Boolean(error)}
                  placeholder={t("git_work_branch_placeholder") || "Leave empty to create a new branch"}
                  className="w-full"
                />
                {error?.message && <span className="text-11 text-danger-primary">{error.message}</span>}
              </>
            )}
          />
        </div>
      )}
    </div>
  );
}
