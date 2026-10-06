/**
 * Whether this is the Tauri desktop build. The desktop overlay replaces this
 * file with one that detects the native bridge; always false everywhere else.
 */
export function isDesktop(): boolean {
  return false;
}

/** Edition seam for preparing a user-triggered agent run. */
export type AgentRunTarget = { workspaceSlug: string; projectId: string; issueId: string };

export async function prepareAgentRun(_target: AgentRunTarget): Promise<void> {}

export async function disposeAgentRuntime(): Promise<void> {}
