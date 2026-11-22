import { type ReactNode, useEffect, useState } from "react";
import { api } from "../api";
import icon from "../assets/icon.jpg";
import { useStore } from "../store";
import { countBusy } from "../tree";
import { busyWarning, updateLine } from "../updates";
import { useWords } from "../useWords";

const SOURCE = "https://github.com/ryckakas/swarmed";
const RELEASES = "https://github.com/ryckakas/swarmed/releases";
const MAKER = "https://kauneckas.dev";

/** What Swarmed is, which version is installed, and where to read more. */
export function About() {
    const version = useStore((state) => state.version);
    const words = useWords();
    return (
        <div className="about">
            <img className="about-icon" src={icon} alt="" width={64} height={64} />
            <div>
                <p className="about-name">Swarmed</p>
                {version && <p className="about-version">{words.versionName(version)}</p>}
            </div>
            <UpdateRow />
            <p className="about-line">{words.aboutLine}</p>
            <p className="about-links">
                <ExternalLink href={SOURCE}>Source on GitHub</ExternalLink>
                <ExternalLink href={RELEASES}>Release notes</ExternalLink>
            </p>
            <p className="about-maker">
                Made by <ExternalLink href={MAKER}>Ricardas Kauneckas</ExternalLink>, free under the
                MIT licence.
            </p>
        </div>
    );
}

/** Looks for a new version, as the About view asks when it is shown. */
export function lookForUpdate() {
    api.updateCheck().then(
        (update) => useStore.setState({ update }),
        (error: unknown) =>
            useStore.setState({ update: { state: "failed", reason: String(error) } }),
    );
}

/**
 * Whether a newer Swarmed is out, and the button that installs it or looks again. Being shown
 * is what makes it look: with automatic updates off, nothing is fetched before.
 */
function UpdateRow() {
    const update = useStore((state) => state.update);
    const busy = useStore((state) => countBusy(state.projects));
    const report = useStore((state) => state.report);
    const words = useWords();
    // Restarting with sessions in a turn is asked about once, here, before it is done.
    const [asking, setAsking] = useState(false);
    useEffect(lookForUpdate, []);
    const restart = () => {
        if (busy > 0 && !asking) {
            setAsking(true);
            return;
        }
        setAsking(false);
        api.updateRestart().catch(report);
    };
    const installs = update.state === "available" || update.state === "ready";
    const looks = update.state === "up_to_date" || update.state === "failed";
    // Each step of a download would be read out: only where updating stands is.
    const announced = update.state === "downloading" ? "off" : "polite";
    return (
        <div className="about-update" aria-live={announced}>
            <p>{updateLine(update, words)}</p>
            {asking && <p className="about-update-warning">{busyWarning(busy)}</p>}
            {installs && (
                <button type="button" className="button button-primary" onClick={restart}>
                    {asking
                        ? "Restart anyway"
                        : update.state === "ready"
                          ? words.update.restart
                          : words.update.install}
                </button>
            )}
            {asking && (
                <button type="button" className="button" onClick={() => setAsking(false)}>
                    Not now
                </button>
            )}
            {looks && (
                <button type="button" className="button" onClick={lookForUpdate}>
                    {words.update.check}
                </button>
            )}
        </div>
    );
}

/** A link that opens in the browser: the window itself never leaves the app. */
function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
    const report = useStore((state) => state.report);
    return (
        <a
            href={href}
            onClick={(event) => {
                event.preventDefault();
                api.openLink(href).catch(report);
            }}
        >
            {children}
        </a>
    );
}
