// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Set-password card. Passwordless accounts set their first secret through
// the shared new-password form: success lands on the entry route and
// failures toast. Who sees this card is the route guard's decision.
// Written from parity/specs/auth.md AUTH-014.

import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useSetPassword } from "../api.js";
import { describeSetPasswordFailure, SET_FAILURE_TITLE } from "../recovery.js";
import { NewPasswordForm } from "./NewPasswordForm.js";

export function SetPasswordCard(): React.ReactElement {
  const setter = useSetPassword();

  const submit = (password: string) => {
    setter.mutate(password, {
      onSuccess: () => {
        globalThis.window.location.assign("/");
      },
      onError: (error) => {
        notifyError(SET_FAILURE_TITLE, describeSetPasswordFailure(error));
      },
    });
  };

  return (
    <section aria-label="Set password" className="flex w-full max-w-90 flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Set your password</h1>
        <p className="text-body text-(--text-muted)">Choose a secret to sign in with next time.</p>
      </div>
      <NewPasswordForm submitLabel="Set password" pending={setter.isPending} onSubmit={submit} />
    </section>
  );
}
