import type { ReactNode } from "react";

/** One setting: its name and what it does, beside the control that changes it. */
export function Row({
    label,
    hint,
    children,
}: {
    label: string;
    hint?: string;
    children: ReactNode;
}) {
    return (
        <div className="settings-row">
            <div>
                <p className="settings-label">{label}</p>
                {hint && <p className="settings-hint">{hint}</p>}
            </div>
            {children}
        </div>
    );
}
