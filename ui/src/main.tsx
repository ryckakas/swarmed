import "@fontsource-variable/atkinson-hyperlegible-next";
import "@fontsource-variable/atkinson-hyperlegible-mono";
// KaTeX's stylesheet and fonts, bundled: the window loads nothing from elsewhere. Before the
// app's own, which draws on top of it. Copying a formula gives its TeX, not its symbols.
import "katex/dist/katex.min.css";
import "katex/contrib/copy-tex";
import "./styles/tokens.css";
import "./styles/palettes.css";
import "./styles/app.css";
import "./styles/layouts.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) {
    throw new Error("index.html has no #root element");
}
createRoot(root).render(
    <StrictMode>
        <App />
    </StrictMode>,
);
