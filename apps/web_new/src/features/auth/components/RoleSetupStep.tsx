// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role-selection onboarding step (AUTH-032). One fixed-list role is
// required to continue, or skip; the flow hides this step entirely on
// self-managed instances. A failed save toasts but still advances.
// Written from parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useProfile } from "../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useUpdateProfile } from "../profile.js";
import { ONBOARDING_ROLES } from "../onboarding.js";

export const ROLE_REQUIRED_MESSAGE = "Choose the role that fits you best.";
export const ROLE_SAVE_SUCCESS_TITLE = "Role saved";
export const ROLE_SAVE_FAILED_TITLE = "Could not save your role";

export interface RoleSetupStepProps {
  onAdvance: () => void;
}

export function RoleSetupStep({ onAdvance }: RoleSetupStepProps): React.ReactElement {
  const { data: profile } = useProfile();
  const updateProfile = useUpdateProfile();

  const [selected, setSelected] = React.useState(profile?.role ?? "");
  const [attempted, setAttempted] = React.useState(false);
  const pending = updateProfile.isPending;

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    if (selected === "") {
      setAttempted(true);
      return;
    }
    try {
      await updateProfile.mutateAsync({ role: selected });
      notifySuccess(ROLE_SAVE_SUCCESS_TITLE);
    } catch {
      notifyError(ROLE_SAVE_FAILED_TITLE);
    }
    onAdvance();
  };

  return (
    <section aria-labelledby="role-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="role-heading" className="text-h2 text-(--text)">
          What&apos;s your role?
        </h1>
        <p className="text-body text-(--text-muted)">Pick the closest match so Pi Dash fits how you work.</p>
      </div>

      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-(--space-4)">
        <div role="radiogroup" aria-label="Role" className="flex flex-col gap-(--space-2)">
          {ONBOARDING_ROLES.map((role, index) => (
            <label
              key={role.id}
              className="text-body flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-3) text-(--text)"
            >
              <input
                type="radio"
                name="onboarding-role"
                value={role.id}
                checked={selected === role.id}
                autoFocus={index === 0}
                onChange={() => setSelected(role.id)}
              />
              {role.label}
            </label>
          ))}
        </div>
        {attempted && selected === "" ? <p className="text-body text-(--danger)">{ROLE_REQUIRED_MESSAGE}</p> : null}

        <div className="flex flex-col gap-(--space-2)">
          <Button type="submit" variant="primary" size="large" loading={pending} disabled={selected === "" || pending}>
            Continue
          </Button>
          <Button type="button" variant="ghost" onClick={onAdvance} disabled={pending}>
            Skip
          </Button>
        </div>
      </form>
    </section>
  );
}
