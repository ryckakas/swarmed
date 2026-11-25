// The Browser pane's logic: where on the page a click on its picture lands, which keys go to
// the page, how big the page is, and what the console shows. Pure, so it can be tested.

import type {
    BrowserEvent,
    BrowserState,
    BrowserTab,
    ConsoleLine,
    KeyInput,
    Modifiers,
    MouseInput,
    Task,
    Viewport,
} from "./api/types";
import type { WidthLimits } from "./resize";

/** The pane may be dragged wide; the stylesheet keeps the conversation readable beside it. */
export const BROWSER_WIDTH: WidthLimits = { initial: 560, min: 320, max: 2400 };

/** The most minutes the settings may leave an unused browser running, as the core allows. */
export const MAX_IDLE_MINUTES = 1440;

/** The minutes typed for stopping an unused browser, within what the core takes. */
export function minutesWithin(typed: string): number {
    const minutes = Math.round(Number(typed));
    if (!Number.isFinite(minutes)) {
        return 30;
    }
    return Math.min(MAX_IDLE_MINUTES, Math.max(0, minutes));
}

/** How many lines of a tab's console the pane keeps, as the core does. */
export const CONSOLE_KEPT = 500;

/** Where the picture is drawn on screen. */
export interface Box {
    left: number;
    top: number;
    width: number;
    height: number;
}

/** The page's own size, in CSS pixels, as its latest picture said. */
export interface PageSize {
    width: number;
    height: number;
}

interface HeldKeys {
    altKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
}

/** A pointer event, as far as the page needs it. */
export interface PointerLike extends HeldKeys {
    clientX: number;
    clientY: number;
    button: number;
    buttons: number;
    detail: number;
}

/** A wheel event, as far as the page needs it. */
export interface WheelLike extends HeldKeys {
    clientX: number;
    clientY: number;
    deltaX: number;
    deltaY: number;
    /** 0 for pixels, 1 for lines, 2 for pages. */
    deltaMode: number;
}

/** A key event, as far as the page needs it. */
export interface KeyLike extends HeldKeys {
    key: string;
    code: string;
    keyCode: number;
    repeat: boolean;
    isComposing: boolean;
}

export function modifiersOf(event: HeldKeys): Modifiers {
    return {
        alt: event.altKey,
        ctrl: event.ctrlKey,
        meta: event.metaKey,
        shift: event.shiftKey,
    };
}

const BUTTONS: MouseInput["button"][] = ["left", "middle", "right", "back", "forward"];

/** Where a point on screen is on the page, in its CSS pixels. */
export function pagePoint(
    clientX: number,
    clientY: number,
    box: Box,
    page: PageSize,
): { x: number; y: number } {
    const across = box.width > 0 ? page.width / box.width : 1;
    const down = box.height > 0 ? page.height / box.height : 1;
    return {
        x: Math.round((clientX - box.left) * across * 10) / 10,
        y: Math.round((clientY - box.top) * down * 10) / 10,
    };
}

/** What a pointer did on the picture, as the page takes it. */
export function mouseInput(
    kind: "down" | "up" | "move",
    event: PointerLike,
    box: Box,
    page: PageSize,
): MouseInput {
    const { x, y } = pagePoint(event.clientX, event.clientY, box, page);
    return {
        kind,
        x,
        y,
        button: kind === "move" ? "none" : (BUTTONS[event.button] ?? "none"),
        buttons: event.buttons,
        click_count: kind === "move" ? 0 : Math.max(1, event.detail),
        delta_x: 0,
        delta_y: 0,
        modifiers: modifiersOf(event),
    };
}

/** How far a line and a page of the wheel scroll, in CSS pixels. */
const LINE = 40;
const PAGE = 800;

/** A scroll of the wheel or the trackpad on the picture, in CSS pixels. */
export function wheelInput(event: WheelLike, box: Box, page: PageSize): MouseInput {
    const unit = [1, LINE, PAGE][event.deltaMode] ?? 1;
    const { x, y } = pagePoint(event.clientX, event.clientY, box, page);
    return {
        kind: "wheel",
        x,
        y,
        button: "none",
        buttons: 0,
        click_count: 0,
        delta_x: event.deltaX * unit,
        delta_y: event.deltaY * unit,
        modifiers: modifiersOf(event),
    };
}

/**
 * A key on the picture as the page takes it, or `null` for one it does not get that way: a
 * key that is part of what an input method composes, which arrives whole when it is done, and
 * ⌘V, whose text the window pastes itself, since Chrome's clipboard is not the user's.
 */
export function keyInput(kind: "down" | "up", event: KeyLike): KeyInput | null {
    if (event.isComposing || event.key === "Process" || event.key === "Dead") {
        return null;
    }
    if (event.metaKey && !event.ctrlKey && event.key.toLowerCase() === "v") {
        return null;
    }
    return {
        kind,
        key: event.key,
        code: event.code,
        key_code: event.keyCode,
        modifiers: modifiersOf(event),
        repeat: event.repeat,
    };
}

/** The room the pane gives the page, in whole CSS pixels, at the screen's density. */
export function viewportOf(width: number, height: number, density: number): Viewport {
    return {
        width: Math.max(100, Math.floor(width)),
        height: Math.max(100, Math.floor(height)),
        scale: Math.min(2, Math.max(1, density || 1)),
    };
}

/** Whether two viewports differ enough to tell Chrome. */
export function sameViewport(one: Viewport | null, other: Viewport): boolean {
    return (
        one !== null &&
        one.width === other.width &&
        one.height === other.height &&
        one.scale === other.scale
    );
}

/** The lines kept and those heard since, in order, each once, the latest kept. */
export function consoleMerged(kept: ConsoleLine[], heard: ConsoleLine[]): ConsoleLine[] {
    const byNumber = new Map<number, ConsoleLine>();
    for (const line of [...kept, ...heard]) {
        byNumber.set(line.n, line);
    }
    const lines = [...byNumber.values()].sort((one, other) => one.n - other.n);
    return lines.slice(-CONSOLE_KEPT);
}

/** How many lines of each level that matters a console holds, for its heading. */
export function consoleCounts(lines: ConsoleLine[]): { errors: number; warnings: number } {
    return {
        errors: lines.filter((line) => line.level === "error").length,
        warnings: lines.filter((line) => line.level === "warning").length,
    };
}

/** What a tab is called in the strip. */
export function tabLabel(tab: BrowserTab): string {
    if (tab.url === "about:blank" || tab.url === "") {
        return "New tab";
    }
    return tab.title || tab.url;
}

/** What the address bar shows for a tab: nothing for an empty one, to type into. */
export function addressOf(tab: BrowserTab | null): string {
    if (tab === null || tab.url === "about:blank") {
        return "";
    }
    return tab.url;
}

/** The tab the pane shows. */
export function shownTab(state: BrowserState | null): BrowserTab | null {
    if (state === null) {
        return null;
    }
    return state.tabs.find((tab) => tab.id === state.shown) ?? null;
}

/** The state an event brings for `task`, if it brings one. */
export function stateOf(event: BrowserEvent, task: number): BrowserState | null {
    if (event.kind !== "state" || event.task_id !== task) {
        return null;
    }
    const { kind: _, ...state } = event;
    return state;
}

/**
 * Whether the browser could not start because none is installed: the pane offers to install
 * one instead of saying the error.
 */
export function isMissing(error: string): boolean {
    return error.includes("no Google Chrome, Chromium, Microsoft Edge or Brave was found");
}

/** A line's time, as the console shows it: `14:03:07`. */
export function lineTime(at: number): string {
    if (at <= 0) {
        return "";
    }
    return new Date(at).toTimeString().slice(0, 8);
}

/**
 * The session to show beside a task's browser when its mark in the sidebar is pressed: the one
 * on screen if it is the task's, else the task's latest; `null` for a task with none open.
 */
export function sessionForBrowser(task: Task, selected: number | null): number | null {
    if (task.sessions.some((session) => session.id === selected)) {
        return selected;
    }
    return task.sessions.at(-1)?.id ?? null;
}
