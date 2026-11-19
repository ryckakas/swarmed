import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";
import { clampWidth, type WidthLimits, widthAfterKey } from "../resize";

/** A panel's width, remembered on this machine under `key`. */
export function useStoredWidth(key: string, limits: WidthLimits) {
    const [width, setWidth] = useState(() => clampWidth(Number(read(key)), limits));
    useEffect(() => {
        try {
            localStorage.setItem(key, String(width));
        } catch {
            // Without storage the width just lasts until the window closes.
        }
    }, [key, width]);
    return [width, (wanted: number) => setWidth(clampWidth(wanted, limits))] as const;
}

function read(key: string): string | null {
    try {
        // Nothing stored must not read as zero, which would clamp to the narrowest width.
        return localStorage.getItem(key) ?? "none";
    } catch {
        return null;
    }
}

interface Props {
    /** What the edge resizes, for screen readers: "Resize sidebar". */
    label: string;
    width: number;
    limits: WidthLimits;
    /** Which side of the window the panel is on; dragging away from it widens the panel. */
    side: "left" | "right";
    onResize(width: number): void;
}

/** The edge of a panel: drag it or use the arrow keys to resize, double-click to reset. */
export function Resizer({ label, width, limits, side, onResize }: Props) {
    const drag = useRef<{ startX: number; startWidth: number } | null>(null);
    const direction = side === "left" ? 1 : -1;

    const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { startX: event.clientX, startWidth: width };
    };
    const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
        if (drag.current) {
            const moved = (event.clientX - drag.current.startX) * direction;
            onResize(drag.current.startWidth + moved);
        }
    };
    const stop = () => {
        drag.current = null;
    };
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        const key = side === "left" ? event.key : mirrored(event.key);
        const wanted = widthAfterKey(key, width, limits);
        if (wanted !== null) {
            event.preventDefault();
            onResize(wanted);
        }
    };

    return (
        // biome-ignore lint/a11y/useSemanticElements: an <hr> cannot be dragged or focused
        <div
            className={`resizer resizer-${side}`}
            role="separator"
            aria-orientation="vertical"
            aria-label={label}
            aria-valuenow={width}
            aria-valuemin={limits.min}
            aria-valuemax={limits.max}
            tabIndex={0}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={stop}
            onPointerCancel={stop}
            onDoubleClick={() => onResize(limits.initial)}
            onKeyDown={onKeyDown}
        />
    );
}

/** On a right-hand panel the left arrow widens it. */
function mirrored(key: string): string {
    return { ArrowLeft: "ArrowRight", ArrowRight: "ArrowLeft" }[key] ?? key;
}
