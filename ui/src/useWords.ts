import { useStore } from "./store";
import { type Words, wordsFor } from "./wording";

/** What things are called in the style the user chose. */
export function useWords(): Words {
    return useStore((state) => wordsFor(state.settings?.style));
}
