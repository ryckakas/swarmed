import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

/** Font files, which the window's policy loads only as files of the app's own. */
const FONT = /\.(woff2?|ttf)$/;

/**
 * KaTeX's stylesheet with its fonts named in `woff2` alone, which WebKit reads. The `woff` and
 * `ttf` copies it also names would be bundled with the app and never loaded.
 */
function katexWoff2Only(): Plugin {
    return {
        name: "katex-woff2-only",
        enforce: "pre",
        transform(code, id) {
            if (!/\/katex\/dist\/katex(\.min)?\.css$/.test(id.split("?")[0] ?? "")) {
                return null;
            }
            return code.replace(/,\s*url\([^)]*\.(woff|ttf)\)\s*format\("(woff|truetype)"\)/g, "");
        },
    };
}

// The front end lives in ui/. Tauri loads the dev server in development and ui/dist in a build.
export default defineConfig({
    root: "ui",
    plugins: [react(), katexWoff2Only()],
    // Tauri reports Rust errors in the same terminal; do not let Vite clear them away.
    clearScreen: false,
    server: { port: 5173, strictPort: true },
    build: {
        outDir: "dist",
        emptyOutDir: true,
        target: "safari16",
        // A font is never inlined as a `data:` address, as Vite does with small files: the
        // window's policy refuses fonts from anywhere but the app (`default-src 'self'`).
        assetsInlineLimit: (file) => (FONT.test(file) ? false : undefined),
    },
    test: { include: ["src/**/*.test.ts"], environment: "node" },
});
