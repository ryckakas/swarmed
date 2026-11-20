import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";

interface Props {
    requestId: string;
    /** What is asked, in a line. */
    title: string;
    /** The change itself, as it would be made. */
    detail: string;
    /** How it was answered; `null` while it waits. */
    accepted: boolean | null;
}

/**
 * A change to Swarmed's own configuration that an agent asked for and only the user can make:
 * a rule for every agent, a command that will run later. It shows exactly what would be kept,
 * and nothing is kept until it is accepted here.
 */
export function ConfigProposal({ requestId, title, detail, accepted }: Props) {
    const report = useStore((state) => state.report);
    // Until the app confirms the answer the buttons show the click was taken.
    const [sent, setSent] = useState<boolean | null>(null);
    if (accepted !== null) {
        return (
            <p className="asked">
                {accepted ? "You accepted" : "You declined"}: {title.replace(/\?.*$/, "")}
                {detail && ": "}
                {detail && <span className="mono">{detail.split("\n")[0]}</span>}
            </p>
        );
    }
    const answer = (yes: boolean) => {
        setSent(yes);
        api.configAnswer(requestId, yes).catch((error: unknown) => {
            setSent(null);
            report(error);
        });
    };
    return (
        <section className="permission" aria-label="Proposed change" aria-busy={sent !== null}>
            <p className="permission-ask">{title}</p>
            {detail && <pre className="permission-what">{detail}</pre>}
            <p className="permission-why">
                The agent proposed this change to Swarmed's settings. It is not in effect unless you
                accept it.
            </p>
            <div className="permission-actions">
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => answer(true)}
                    disabled={sent !== null}
                >
                    {sent === true ? "Accepting…" : "Accept"}
                </button>
                <button
                    type="button"
                    className="button"
                    onClick={() => answer(false)}
                    disabled={sent !== null}
                >
                    {sent === false ? "Declining…" : "Decline"}
                </button>
            </div>
        </section>
    );
}
