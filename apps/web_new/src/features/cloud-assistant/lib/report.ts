// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in outcome copy (parity/specs/cloud.md §7, CLOUD-033). Each way the
// provider flow can end gets its own title and body, so "allow popups" is
// never shown for a provider-side refusal, a timeout never reads as a
// failure the provider reported, and a window closed before OpenAI answered
// is not reported as an OpenAI failure.

import {
  POPUP_BLOCKED_BODY,
  POPUP_BLOCKED_TITLE,
  SIGN_IN_CLOSED_BODY,
  SIGN_IN_CLOSED_TITLE,
  SIGN_IN_FAILED_BODY,
  SIGN_IN_FAILED_TITLE,
  SIGN_IN_OK_BODY,
  SIGN_IN_OK_TITLE,
  SIGN_IN_START_FAILED_FALLBACK,
  SIGN_IN_START_FAILED_TITLE,
  SIGN_IN_TIMEOUT_BODY,
  SIGN_IN_TIMEOUT_TITLE,
} from "./copy.js";
import { actionFailureDetail } from "../api/mutations.js";
import type { SignInFinish, SignInStartFailure } from "./signIn.js";

export interface Notice {
  title: string;
  body: string;
}

export function startFailureNotice(failure: SignInStartFailure): Notice {
  switch (failure.kind) {
    case "blocked":
      return { title: POPUP_BLOCKED_TITLE, body: POPUP_BLOCKED_BODY };
    case "closed":
      return { title: SIGN_IN_CLOSED_TITLE, body: SIGN_IN_CLOSED_BODY };
    case "rejected":
      return {
        title: SIGN_IN_START_FAILED_TITLE,
        body: actionFailureDetail(failure.error, SIGN_IN_START_FAILED_FALLBACK),
      };
  }
}

export function finishNotice(finish: SignInFinish): Notice {
  switch (finish.kind) {
    case "confirmed":
      return { title: SIGN_IN_OK_TITLE, body: SIGN_IN_OK_BODY };
    case "timeout":
      return { title: SIGN_IN_TIMEOUT_TITLE, body: SIGN_IN_TIMEOUT_BODY };
    case "failed":
      return { title: SIGN_IN_FAILED_TITLE, body: finish.detail ?? SIGN_IN_FAILED_BODY };
  }
}
