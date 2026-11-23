import { useRef, useState } from "react";
import { api } from "../api";
import type { AgentEvent, Session } from "../api/types";
import { type Remote, remoteOf, remoteSummary, remoteUrl, remoteWanted } from "../remote";
import { RemoteIcon } from "./IconButton";
import { useDismiss } from "./UsageButton";

/** What Remote Control is, said where it is turned on. */
const ABOUT =
    "Carry on with this session from claude.ai or the Claude app on your phone. What you type there is answered here, in this worktree, and shown in this conversation. Claude Code has to be signed in with a claude.ai subscription. While it is on, the conversation is kept on Anthropic's servers as well.";

/**
 * The phone in a Claude Code session's header: it turns Remote Control on and off, and says
 * where the session can be reached. Marked while it is wanted, so that a session reachable
 * from elsewhere is told apart at a glance.
 */
export function RemoteButton({ session, events }: { session: Session; events: AgentEvent[] }) {
    const [open, setOpen] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const wrap = useRef<HTMLDivElement>(null);
    useDismiss(wrap, open, setOpen);
    const remote = remoteOf(events);
    const turn = (on: boolean) => {
        setError(null);
        api.sessionRemoteControl(session.id, on).catch((failed: unknown) => {
            setError(String(failed));
        });
    };
    const label = `Remote Control: ${remoteSummary(remote)}`;
    return (
        <div className="remote-wrap" ref={wrap}>
            <button
                type="button"
                className="icon-button remote-button"
                aria-label={label}
                title={label}
                aria-expanded={open}
                aria-haspopup="dialog"
                data-state={remoteWanted(remote) ? remote?.state : undefined}
                onClick={() => setOpen(!open)}
            >
                <svg viewBox="0 0 16 16" aria-hidden="true">
                    <RemoteIcon />
                </svg>
            </button>
            {open && (
                <div className="remote-panel" role="dialog" aria-label="Remote Control">
                    <p className="remote-panel-title">
                        Remote Control
                        <span>{remoteSummary(remote)}</span>
                    </p>
                    <RemotePanel remote={remote} turn={turn} />
                    {error && <p className="remote-error">{error}</p>}
                </div>
            )}
        </div>
    );
}

/** What the panel holds: where the session is and the switch, or what Remote Control is. */
function RemotePanel({ remote, turn }: { remote: Remote | null; turn: (on: boolean) => void }) {
    const url = remoteUrl(remote);
    const wanted = remoteWanted(remote);
    return (
        <>
            {url ? <RemoteAddress url={url} /> : <p className="remote-about">{ABOUT}</p>}
            {remote?.state === "paused" && (
                <p className="remote-about">
                    The agent is not running. It starts again with your next message here, or with
                    Reconnect.
                </p>
            )}
            <div className="remote-panel-foot">
                {remote?.state === "paused" && (
                    <button type="button" className="button" onClick={() => turn(true)}>
                        Reconnect
                    </button>
                )}
                <button
                    type="button"
                    className={wanted ? "button" : "button button-primary"}
                    onClick={() => turn(!wanted)}
                >
                    {wanted ? "Turn off" : remote?.state === "failed" ? "Try again" : "Turn on"}
                </button>
            </div>
        </>
    );
}

/** Where the session is on claude.ai, to open there or copy for the phone. */
function RemoteAddress({ url }: { url: string }) {
    const [copied, setCopied] = useState(false);
    const copy = () => {
        navigator.clipboard
            .writeText(url)
            .then(() => setCopied(true))
            .catch(() => setCopied(false));
    };
    return (
        <>
            <p className="remote-about">
                Open it in the Claude app, where it is listed with your other sessions, or at:
            </p>
            <p className="remote-url">
                <code>{url}</code>
            </p>
            <div className="remote-panel-foot">
                <button type="button" className="button" onClick={copy}>
                    {copied ? "Copied" : "Copy link"}
                </button>
                <button type="button" className="button" onClick={() => void api.openLink(url)}>
                    Open in browser
                </button>
            </div>
        </>
    );
}
