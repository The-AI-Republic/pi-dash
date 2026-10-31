// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Strength hint beside the sign-up secret field (AUTH-010). Lists what a
// strong secret needs, marking the rules the current value already meets.
// The card shows it while the secret field holds a weak value.

import { Check, Minus } from "lucide-react";
import * as React from "react";

import { secretCriteria } from "../passwordStrength.js";

export function PasswordStrengthHint({ secret }: { secret: string }): React.ReactElement {
  const criteria = secretCriteria(secret);
  return (
    <div role="group" aria-label="Password strength" className="flex flex-col gap-(--space-1)">
      <ul className="flex flex-col gap-(--space-1)">
        {criteria.map((criterion) => (
          <li
            key={criterion.id}
            className={`text-meta flex items-center gap-(--space-1) ${
              criterion.met ? "text-(--success)" : "text-(--text-muted)"
            }`}
          >
            {criterion.met ? <Check size={12} aria-hidden="true" /> : <Minus size={12} aria-hidden="true" />}
            {criterion.hint}
          </li>
        ))}
      </ul>
    </div>
  );
}
