import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    // bases/ 是基座评估 checkout（含各自 node_modules），不参与本仓库测试
    exclude: ["bases/**", "**/node_modules/**"],
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
