/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { AuthRoot } from "@/components/account/auth-forms/auth-root";
import type { EAuthModes } from "@/helpers/authentication.helper";
import { AuthHero } from "./auth-hero";
import { AuthHeader } from "./header";

type AuthBaseProps = {
  authType: EAuthModes;
};

export function AuthBase({ authType }: AuthBaseProps) {
  return (
    <div className="relative z-10 flex h-screen w-screen flex-col items-center overflow-hidden overflow-y-auto px-8 pt-6 pb-10">
      <div className="mx-auto flex w-full max-w-[60rem] flex-grow flex-col">
        <AuthHeader type={authType} />
        <div className="flex w-full flex-grow flex-col items-center justify-center gap-12 lg:flex-row lg:justify-center lg:gap-20">
          <AuthHero className="order-2 lg:order-1" />
          <div className="order-1 flex w-full max-w-[22.5rem] flex-shrink-0 flex-col justify-center lg:order-2">
            <AuthRoot authMode={authType} />
          </div>
        </div>
      </div>
    </div>
  );
}
