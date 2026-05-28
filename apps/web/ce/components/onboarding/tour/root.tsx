/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// pi dash imports
import { Button } from "@pi-dash/propel/button";
import { CloseIcon, PiSymbol } from "@pi-dash/propel/icons";
// assets
// TODO(onboarding): replace these reused Plane-era screenshots with Pi Dash-native captures
// (runner CLI, runner chat, workpad) once design provides them.
import CyclesTour from "@/app/assets/onboarding/cycles.webp?url";
import IssuesTour from "@/app/assets/onboarding/issues.webp?url";
import ModulesTour from "@/app/assets/onboarding/modules.webp?url";
import PagesTour from "@/app/assets/onboarding/pages.webp?url";
import ViewsTour from "@/app/assets/onboarding/views.webp?url";
// hooks
import { useCommandPalette } from "@/hooks/store/use-command-palette";
import { useUser } from "@/hooks/store/user";
// local imports
import { TourSidebar } from "./sidebar";

export type TOnboardingTourProps = {
  onComplete: () => void;
};

export type TTourSteps = "welcome" | "work-items" | "runners" | "chat" | "workpads" | "pages";

const TOUR_STEPS: {
  key: TTourSteps;
  title: string;
  description: string;
  image: string;
  prevStep?: TTourSteps;
  nextStep?: TTourSteps;
}[] = [
  {
    key: "work-items",
    title: "Plan work your agents can run",
    description:
      "Work items are the unit of work in Pi Dash. Describe what needs to happen, attach context, and hand it to a coding agent — or keep it for your team.",
    image: IssuesTour,
    nextStep: "runners",
  },
  {
    key: "runners",
    title: "Connect a Pi Dash Runner",
    description:
      "Install the Pi Dash Runner on your dev machine to plug in Claude Code or Codex. The runner picks up work items you dispatch and executes them in the background.",
    image: CyclesTour,
    prevStep: "work-items",
    nextStep: "chat",
  },
  {
    key: "chat",
    title: "Steer agents from Runner Chat",
    description:
      "Talk to your agents while they work. Answer questions, redirect mid-task, or review what they are doing — all in a live chat tied to each runner session.",
    image: ModulesTour,
    prevStep: "runners",
    nextStep: "workpads",
  },
  {
    key: "workpads",
    title: "Collaborate in the workpad",
    description:
      "Every work item has a workpad — a shared Markdown space where you and your agent build context together. Plans, decisions, and handoff notes stay with the issue.",
    image: ViewsTour,
    prevStep: "chat",
    nextStep: "pages",
  },
  {
    key: "pages",
    title: "Document with pages",
    description:
      "Use Pages for specs, runbooks, and notes your team and your agents can reference. A good place to capture context that spans multiple work items.",
    image: PagesTour,
    prevStep: "workpads",
  },
];

export const TourRoot = observer(function TourRoot(props: TOnboardingTourProps) {
  const { onComplete } = props;
  // states
  const [step, setStep] = useState<TTourSteps>("welcome");
  // store hooks
  const { toggleCreateProjectModal } = useCommandPalette();
  const { data: currentUser } = useUser();

  const currentStepIndex = TOUR_STEPS.findIndex((tourStep) => tourStep.key === step);
  const currentStep = TOUR_STEPS[currentStepIndex];

  return (
    <>
      {step === "welcome" ? (
        <div className="w-4/5 overflow-hidden rounded-[10px] bg-surface-1 md:w-1/2 lg:w-2/5">
          <div className="h-full overflow-hidden">
            <div className="grid h-64 place-items-center bg-accent-primary">
              <PiSymbol className="h-20 w-auto text-on-color" />
            </div>
            <div className="flex flex-col overflow-y-auto p-6">
              <h3 className="font-semibold sm:text-18">
                Welcome to Pi Dash, {currentUser?.first_name} {currentUser?.last_name}
              </h3>
              <p className="mt-3 text-13 text-secondary">
                Pi Dash is where you plan work and dispatch it to AI coding agents. Define what needs to be done,
                connect a runner, and watch your agents execute it — with you in the loop the whole way.
              </p>
              <div className="flex h-full items-end">
                <div className="mt-12 flex items-center gap-6">
                  <Button
                    variant="primary"
                    onClick={() => {
                      setStep("work-items");
                    }}
                  >
                    Take a Product Tour
                  </Button>
                  <button
                    type="button"
                    className="bg-transparent text-11 font-medium text-accent-primary outline-subtle-1"
                    onClick={() => {
                      onComplete();
                    }}
                  >
                    No thanks, I will explore it myself
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="relative grid h-3/5 w-4/5 grid-cols-10 overflow-hidden rounded-[10px] bg-surface-1 sm:h-3/4 md:w-1/2 lg:w-3/5">
          <button
            type="button"
            className="fixed top-[19%] right-[9%] z-10 translate-x-1/2 -translate-y-1/2 cursor-pointer rounded-full border border-strong p-1 sm:top-[11.5%] md:right-[24%] lg:right-[19%]"
            onClick={onComplete}
          >
            <CloseIcon className="border-strong- h-3 w-3 text-primary" />
          </button>
          <TourSidebar step={step} setStep={setStep} />
          <div className="col-span-10 h-full overflow-hidden lg:col-span-7">
            <div
              className={`flex h-1/2 items-end overflow-hidden bg-accent-primary sm:h-3/5 ${
                currentStepIndex % 2 === 0 ? "justify-end" : "justify-start"
              }`}
            >
              <img src={currentStep?.image} className="h-full w-full object-cover" alt={currentStep?.title} />
            </div>
            <div className="flex h-1/2 flex-col overflow-y-auto p-4 sm:h-2/5">
              <h3 className="font-semibold sm:text-18">{currentStep?.title}</h3>
              <p className="mt-3 text-13 text-secondary">{currentStep?.description}</p>
              <div className="mt-3 flex h-full items-end justify-between gap-4">
                <div className="flex items-center gap-4">
                  {currentStep?.prevStep && (
                    <Button variant="secondary" onClick={() => setStep(currentStep.prevStep ?? "welcome")}>
                      Back
                    </Button>
                  )}
                  {currentStep?.nextStep && (
                    <Button variant="primary" onClick={() => setStep(currentStep.nextStep ?? "work-items")}>
                      Next
                    </Button>
                  )}
                </div>
                {currentStepIndex === TOUR_STEPS.length - 1 && (
                  <Button
                    variant="primary"
                    onClick={() => {
                      onComplete();
                      toggleCreateProjectModal(true);
                    }}
                  >
                    Create your first project
                  </Button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
});
