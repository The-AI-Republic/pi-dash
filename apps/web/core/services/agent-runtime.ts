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

/** What the user chose when confirming a sign-out. */
export type SignOutChoice = { deleteChatHistory: boolean };

/**
 * Edition seam for confirming a user-initiated sign-out. The desktop asks, and
 * offers to delete the account's local chat history; resolves `null` when the
 * user cancels. The web has nothing local to ask about.
 */
export async function confirmSignOut(): Promise<SignOutChoice | null> {
  return { deleteChatHistory: false };
}

export async function disposeAgentRuntime(_choice?: SignOutChoice): Promise<void> {}
