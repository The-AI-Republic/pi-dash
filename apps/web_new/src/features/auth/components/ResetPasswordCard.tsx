// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reset-password card. The address shows locked with the shared
// new-password form behind it; submitting posts the native reset form and
// returns to the sign-in entry on success. Dead links (invalid, expired,
// consumed, or unparseable) render the explanatory banner instead of the
// form. Written from parity/specs/auth.md AUTH-013.

import * as React from "react";

import { usePasswordReset } from "../api.js";
import { describeResetException, describeResetFailure, INVALID_TOKEN_BANNER } from "../recovery.js";
import { NewPasswordForm } from "./NewPasswordForm.js";
import { AuthBanner } from "./SignInCard.js";

export interface ResetPasswordCardProps {
  uid: string;
  token: string;
  email?: string | undefined;
}

export function ResetPasswordCard({ uid, token, email }: ResetPasswordCardProps): React.ReactElement {
  const reset = usePasswordReset();
  const [banner, setBanner] = React.useState<string | null>(null);
  const [tokenDead, setTokenDead] = React.useState(false);

  // A link without its params can never submit, so it banners at once.
  const linkComplete = uid.length > 0 && token.length > 0;

  const submit = (password: string) => {
    reset.mutate(
      { uid, token, password, confirmPassword: password },
      {
        onSuccess: (result) => {
          if (result.ok) {
            globalThis.window.location.assign("/");
          } else {
            const failure = describeResetFailure(result);
            setBanner(failure.message);
            setTokenDead(failure.tokenDead);
          }
        },
        onError: (error) => {
          const failure = describeResetException(error);
          setBanner(failure.message);
          setTokenDead(failure.tokenDead);
        },
      }
    );
  };

  if (!linkComplete || tokenDead) {
    return (
      <section aria-label="Reset password" className="flex w-full max-w-90 flex-col gap-(--space-6)">
        <div className="flex flex-col gap-(--space-1)">
          <h1 className="text-h2 text-(--text)">Reset your password</h1>
          <p className="text-body text-(--text-muted)">This reset link is no longer usable.</p>
        </div>
        <AuthBanner message={banner ?? INVALID_TOKEN_BANNER} />
      </section>
    );
  }

  return (
    <section aria-label="Reset password" className="flex w-full max-w-90 flex-col gap-(--space-6)">
      {banner ? <AuthBanner message={banner} /> : null}
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Choose a new password</h1>
        <p className="text-body text-(--text-muted)">Pick a strong secret to finish resetting.</p>
      </div>
      <NewPasswordForm email={email} submitLabel="Reset password" pending={reset.isPending} onSubmit={submit} />
    </section>
  );
}
