// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Use-case onboarding step (AUTH-033). At least one shared-list use case
// is required to continue, or skip; the selection saves as one combined
// profile value. The flow hides this step entirely on self-managed
// instances. A failed save toasts but still advances. Written from
// parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

import { useProfile } from "../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useUpdateProfile } from "../profile.js";
import { combineUseCases, ONBOARDING_USE_CASES, parseUseCases } from "../onboarding.js";

export const USE_CASE_REQUIRED_MESSAGE = "Choose at least one use case.";
export const USE_CASE_SAVE_SUCCESS_TITLE = "Use cases saved";
export const USE_CASE_SAVE_FAILED_TITLE = "Could not save your use cases";

export interface UseCaseSetupStepProps {
  onAdvance: () => void;
}

export function UseCaseSetupStep({ onAdvance }: UseCaseSetupStepProps): React.ReactElement {
  const { data: profile } = useProfile();
  const updateProfile = useUpdateProfile();

  const [selected, setSelected] = React.useState<readonly string[]>(() => parseUseCases(profile?.use_case));
  const [attempted, setAttempted] = React.useState(false);
  const pending = updateProfile.isPending;

  const toggle = (useCase: string): void => {
    setSelected((current) =>
      current.includes(useCase) ? current.filter((entry) => entry !== useCase) : [...current, useCase]
    );
  };

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    if (selected.length === 0) {
      setAttempted(true);
      return;
    }
    try {
      await updateProfile.mutateAsync({ use_case: combineUseCases(selected) });
      notifySuccess(USE_CASE_SAVE_SUCCESS_TITLE);
    } catch {
      notifyError(USE_CASE_SAVE_FAILED_TITLE);
    }
    onAdvance();
  };

  return (
    <section aria-labelledby="use-case-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="use-case-heading" className="text-h2 text-(--text)">
          What brings you to Pi Dash?
        </h1>
        <p className="text-body text-(--text-muted)">Choose everything you plan to use Pi Dash for.</p>
      </div>

      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-(--space-4)">
        <div role="group" aria-label="Use cases" className="flex flex-col gap-(--space-2)">
          {ONBOARDING_USE_CASES.map((useCase, index) => (
            <label
              key={useCase}
              className="text-body flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-3) text-(--text)"
            >
              <input
                type="checkbox"
                checked={selected.includes(useCase)}
                autoFocus={index === 0}
                onChange={() => toggle(useCase)}
              />
              {useCase}
            </label>
          ))}
        </div>
        {attempted && selected.length === 0 ? (
          <p className="text-body text-(--danger)">{USE_CASE_REQUIRED_MESSAGE}</p>
        ) : null}

        <div className="flex flex-col gap-(--space-2)">
          <Button
            type="submit"
            variant="primary"
            size="large"
            loading={pending}
            disabled={selected.length === 0 || pending}
          >
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
