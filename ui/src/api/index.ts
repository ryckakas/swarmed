import type { Api } from "./client";
import { createMockApi } from "./mock";
import { tauriApi } from "./tauri";

/** Tauri injects this global into its webview; a plain browser tab does not have it. */
const insideTauri = "__TAURI_INTERNALS__" in globalThis;

export const api: Api = insideTauri ? tauriApi : createMockApi();
export type { Api } from "./client";
export type * from "./types";
