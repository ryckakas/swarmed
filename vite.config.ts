import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The front end lives in ui/. Tauri loads the dev server in development and ui/dist in a build.
export default defineConfig({
    root: "ui",
    plugins: [react()],
    // Tauri reports Rust errors in the same terminal; do not let Vite clear them away.
    clearScreen: false,
    server: { port: 5173, strictPort: true },
    build: { outDir: "dist", emptyOutDir: true, target: "safari16" },
    test: { include: ["src/**/*.test.ts"], environment: "node" },
});
