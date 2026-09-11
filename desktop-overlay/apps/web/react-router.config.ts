import type { Config } from "@react-router/dev/config";

// Desktop bundle: plain client-side SPA. An edition's web config may
// prerender public routes for crawlers; those routes don't exist in this
// bundle (see app/routes/extended.ts) and there is nothing to index inside
// a Tauri webview.
export default {
  appDirectory: "app",
  ssr: false,
} satisfies Config;
