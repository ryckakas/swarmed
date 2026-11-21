import type { ReactNode } from "react";
import { api } from "../api";
import icon from "../assets/icon.jpg";
import { useStore } from "../store";
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
                {version && <p className="about-version">Version {version}</p>}
            </div>
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
