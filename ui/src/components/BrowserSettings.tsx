import { useEffect, useState } from "react";
import { api } from "../api";
import type { BrowserDisplay, BrowserSetup, Settings } from "../api/types";
import { MAX_IDLE_MINUTES, minutesWithin } from "../browser";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { Picker } from "./Picker";
import { Row } from "./SettingsRow";

/** Where a task's browser can be shown, as the menu names each. */
const DISPLAYS: [BrowserDisplay, string][] = [
    ["pane", "In the pane"],
    ["window", "In a window of its own"],
];

/** Which browser the tasks' own Chrome runs on, and when one that sits unused is stopped. */
export function BrowserSettings({ settings }: { settings: Settings }) {
    const words = useWords();
    const change = useStore((state) => state.changeSettings);
    const [setup, setSetup] = useState<BrowserSetup | null>(null);
    const [program, setProgram] = useState(settings.browser_program);
    const [minutes, setMinutes] = useState(String(settings.browser_idle_minutes));
    // biome-ignore lint/correctness/useExhaustiveDependencies: looked for again when the program changes
    useEffect(() => {
        api.browserSetup().then(setSetup, () => setSetup(null));
    }, [settings.browser_program]);
    const keepProgram = () => {
        if (program.trim() !== settings.browser_program) {
            void change(api, { browser_program: program.trim() });
        }
    };
    const keepMinutes = () => {
        const kept = minutesWithin(minutes);
        setMinutes(String(kept));
        if (kept !== settings.browser_idle_minutes) {
            void change(api, { browser_idle_minutes: kept });
        }
    };
    const found = setup?.name ? `Runs on ${setup.name}.` : "None was found on this machine.";
    return (
        <section className="settings-group" aria-label="Browser">
            <h3>Browser</h3>
            <p className="aside">
                Each {words.terms.task} can have a Chrome of its own, shown beside its sessions,
                with a profile of its own: what is signed in to there stays with the{" "}
                {words.terms.task}, and goes when it is removed.
            </p>
            <Row
                label="Program"
                hint={`${found} Empty: the first of Google Chrome, Chromium, Microsoft Edge and Brave that is installed.`}
            >
                <input
                    className="key-input mono browser-program"
                    aria-label="The browser's program"
                    placeholder="Found by itself"
                    spellCheck={false}
                    value={program}
                    onChange={(event) => setProgram(event.target.value)}
                    onBlur={keepProgram}
                    onKeyDown={(event) => event.key === "Enter" && keepProgram()}
                />
            </Row>
            <Row
                label="Show it"
                hint={`In the pane, Swarmed draws pictures of the page beside the conversation. In a window of its own, Chrome draws the page itself, as sharp and smooth as any Chrome, and the pane lists its tabs and brings the window forward. Either way it is the same Chrome, and agents' browser tools act in it. A browser that runs keeps its way until it starts again.`}
            >
                <Picker
                    label="Where a task's browser is shown"
                    value={settings.browser_display}
                    onChange={(value) =>
                        void change(api, { browser_display: value as BrowserDisplay })
                    }
                    options={DISPLAYS}
                />
            </Row>
            <Row
                label="Stop when unused"
                hint={`A ${words.terms.task}'s browser that nobody looked at or used for this many minutes is stopped; 0 to leave it running until the ${words.terms.task} is put away or Swarmed quits. It starts again when its pane opens. One in a window of its own is never stopped for this.`}
            >
                <span className="rule-days">
                    <input
                        className="key-input mono rule-days-count"
                        aria-label="Stop a browser unused for this many minutes"
                        type="number"
                        inputMode="numeric"
                        min={0}
                        max={MAX_IDLE_MINUTES}
                        step={5}
                        value={minutes}
                        onChange={(event) => setMinutes(event.target.value)}
                        onBlur={keepMinutes}
                        onKeyDown={(event) => event.key === "Enter" && keepMinutes()}
                    />
                    <span className="rule-days-unit">minutes</span>
                </span>
            </Row>
        </section>
    );
}
