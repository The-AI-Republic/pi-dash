import { WEB_BASE_URL } from "@pi-dash/constants";

/**
 * Resolve an in-app path to the hosted Pi Dash frontend.
 *
 * Bundled desktop pages run on a Tauri-owned origin, so browser-relative
 * sharing helpers otherwise expose `tauri://localhost` (or Tauri's Windows
 * loopback origin). Desktop builds must always provide the public frontend
 * origin through VITE_WEB_BASE_URL.
 */
export const desktopWebUrl = (path: string, webBaseUrl = WEB_BASE_URL): string => {
  const baseUrl = webBaseUrl.trim();
  if (!baseUrl) throw new Error("VITE_WEB_BASE_URL is required to create a shareable desktop link");

  const parsedBaseUrl = new URL(baseUrl);
  if (parsedBaseUrl.protocol !== "http:" && parsedBaseUrl.protocol !== "https:") {
    throw new Error("VITE_WEB_BASE_URL must use http or https");
  }

  return new URL(path, `${baseUrl.replace(/\/+$/, "")}/`).toString();
};
