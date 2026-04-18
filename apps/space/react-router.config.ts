import type { Config } from "@react-router/dev/config";
import { joinUrlPath } from "@apple-pi-dash/utils";

const basePath = joinUrlPath(process.env.VITE_SPACE_BASE_PATH ?? "", "/") ?? "/";

export default {
  appDirectory: "app",
  basename: basePath,
  ssr: true,
} satisfies Config;
