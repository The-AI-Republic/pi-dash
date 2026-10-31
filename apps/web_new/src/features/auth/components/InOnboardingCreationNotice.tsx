// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// In-onboarding creation-disabled notice (AUTH-041). When the instance
// config disables workspace creation, the onboarding create view is
// replaced by this inline notice steering to joining, with a single
// action into the join-by-email view. Written from parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

export const CREATION_NOTICE_JOIN_LABEL = "Join by admin email";

export interface InOnboardingCreationNoticeProps {
  onJoin: () => void;
}

export function InOnboardingCreationNotice({ onJoin }: InOnboardingCreationNoticeProps): React.ReactElement {
  return (
    <section aria-labelledby="creation-notice-heading" className="flex flex-col gap-(--space-6)">
      <h1 id="creation-notice-heading" className="text-h2 text-center text-(--text)">
        Workspace creation is turned off
      </h1>
      <p className="text-body text-center text-(--text-muted)">
        Your instance admin has turned off workspace creation. Ask a workspace owner or admin to invite you, then come
        back here to join — or request access with an admin address below.
      </p>
      <Button type="button" variant="primary" size="large" onClick={onJoin}>
        {CREATION_NOTICE_JOIN_LABEL}
      </Button>
    </section>
  );
}
