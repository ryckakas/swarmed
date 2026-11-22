import { useMemo } from "react";
import type { AgentEvent, Session } from "../api/types";
import { useStore } from "../store";
import { answerOfSubAgent, eventsOfSubAgent, subAgentOf } from "../subagents";
import { useWords } from "../useWords";
import { Markdown } from "./Markdown";
import { Transcript } from "./Transcript";

interface Props {
    session: Session;
    /** Where the session is: its repository and task. */
    crumbs: string;
    /** The tool call that started the sub-agent. */
    id: string;
    /** The session's events, the sub-agent's among them. */
    events: AgentEvent[];
}

/**
 * What one sub-agent of a session says and does, in place of the session's conversation.
 * It is only to be read: a sub-agent answers to the agent that started it, not to the user.
 */
export function SubAgentPane({ session, crumbs, id, events }: Props) {
    const words = useWords();
    const closeSub = useStore((state) => state.closeSub);
    const subAgent = subAgentOf(events, id);
    const own = useMemo(() => eventsOfSubAgent(events, id), [events, id]);
    const answer = answerOfSubAgent(events, id);
    const working = answer === null && session.status !== "idle" && session.status !== "errored";
    return (
        <article className="session sub-agent">
            <header className="session-head">
                <p className="crumbs">
                    {crumbs} / {session.title || words.untitled}
                </p>
                <h2 className="sub-agent-name">{subAgent?.name || words.subAgent}</h2>
                <div className="session-meta">
                    <p className="sub-agent-state" data-working={working}>
                        {words.subAgent}
                        {subAgent?.kind ? `, ${subAgent.kind}` : ""}:{" "}
                        {working ? words.subAgentWorking : words.subAgentDone}
                    </p>
                    <button type="button" className="button" onClick={closeSub}>
                        {words.backToSession}
                    </button>
                </div>
            </header>
            {subAgent?.prompt && (
                <details className="sub-agent-asked">
                    <summary>What it was asked</summary>
                    <Markdown text={subAgent.prompt} streaming={false} />
                </details>
            )}
            {own.length === 0 && answer === null && (
                <p className="aside loading">{words.subAgentSilent}</p>
            )}
            {own.length > 0 && (
                <Transcript
                    key={`sub-${session.id}-${id}`}
                    sessionId={session.id}
                    events={own}
                    working={working}
                />
            )}
            {answer !== null && (
                <section className="sub-agent-answer" aria-label={words.subAgentAnswer}>
                    <p className="sub-agent-answer-head">{words.subAgentAnswer}</p>
                    <Markdown text={answer} streaming={false} />
                </section>
            )}
        </article>
    );
}
