import { useState } from "react";
import { useStore } from "../store";
import { dictionary, searchTerms } from "../wording";

/**
 * The swarm's words, each with the plain word it stands for and what that is, for the style
 * chosen: the colony's names more than the swarm's. Made from the words themselves.
 */
export function SwarmDictionary() {
    const style = useStore((state) => state.settings?.style);
    const [query, setQuery] = useState("");
    const terms = searchTerms(dictionary(style), query);
    return (
        <section className="settings-group" aria-label="Swarm dictionary">
            <h3>Swarm dictionary</h3>
            <p className="settings-hint">
                What the swarm's words stand for, in plain words. The agents are only ever given the
                plain ones.
            </p>
            <input
                className="dictionary-search"
                type="search"
                aria-label="Search the swarm dictionary"
                placeholder="Search a word, or what it means"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
            />
            {terms.length === 0 ? (
                <p className="settings-hint">No word matches “{query.trim()}”.</p>
            ) : (
                <dl className="dictionary">
                    {terms.map((term) => (
                        <div key={`${term.swarm}-${term.plain}`} className="dictionary-entry">
                            <dt>{term.swarm}</dt>
                            <dd>
                                <span className="dictionary-plain">{term.plain}</span>
                                {term.meaning}
                            </dd>
                        </div>
                    ))}
                </dl>
            )}
        </section>
    );
}
