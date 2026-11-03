// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-up card. The same email-first machine as the sign-in card with
// the sign-up home: a new address reaches the password step with a
// confirmation field (or the code step when the account uses code
// login), and clearing the email resets to /sign-up. Written from
// parity/specs/auth.md AUTH-009-011.

import * as React from "react";

import { SignInCard, type SignInCardProps } from "./SignInCard.js";

export type SignUpCardProps = Omit<SignInCardProps, "homeMode">;

export function SignUpCard(props: SignUpCardProps): React.ReactElement {
  return <SignInCard homeMode="sign-up" {...props} />;
}
