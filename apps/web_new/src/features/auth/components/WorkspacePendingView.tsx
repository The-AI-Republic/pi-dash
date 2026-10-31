// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace holding sub-view (AUTH-036). The pending-approval parking
// spot after an access request: it names the admin address the request
// went to, and its only action returns to the create view. Written from
// parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

export interface WorkspacePendingViewProps {
  adminEmail: string;
  onCreateInstead: () => void;
}

export function WorkspacePendingView({ adminEmail, onCreateInstead }: WorkspacePendingViewProps): React.ReactElement {
  return (
    <section aria-labelledby="pending-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="pending-heading" className="text-h2 text-(--text)">
          Your request is pending
        </h1>
        {adminEmail === "" ? (
          <p className="text-body text-(--text-muted)">
            Your request to join is waiting for approval. You will join automatically once it is approved.
          </p>
        ) : (
          <p className="text-body text-(--text-muted)">
            We sent your request to <span className="font-medium text-(--text)">{adminEmail}</span>. You will join
            automatically once they approve.
          </p>
        )}
      </div>

      <Button type="button" variant="ghost" size="large" onClick={onCreateInstead}>
        Create a workspace instead
      </Button>
    </section>
  );
}
