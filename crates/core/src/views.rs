//! HTML an agent wrote for showing, held until the window's frame fetches it.
//!
//! A front end hands a finished document over and gets an id back; the shell serves the
//! document from an origin of its own, away from Swarmed's commands. Nothing here is stored:
//! a document can always be handed over again from the transcript it came from.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::Result;
use crate::{Core, lock, open};

/// What a document an agent wrote may do wherever it is shown: run its own inline code and
/// styles, and load nothing from anywhere.
pub const VIEW_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; \
    script-src 'unsafe-inline'; img-src data:; font-src data:; form-action 'none'; base-uri 'none'";

/// Where `tag`, such as `<head`, ends in `document`, if it is there. Tag names are matched
/// whatever their case.
fn after_tag(document: &str, tag: &str) -> Option<usize> {
    let start = document.to_ascii_lowercase().find(tag)?;
    Some(start + document[start..].find('>')? + 1)
}

/// `document` carrying [`VIEW_POLICY`] inside it, for when no server is there to send it as a
/// header: a browser reading the file applies it all the same.
fn with_policy(document: &str) -> String {
    let policy = format!(r#"<meta http-equiv="Content-Security-Policy" content="{VIEW_POLICY}">"#);
    if let Some(at) = after_tag(document, "<head") {
        return format!("{}{policy}{}", &document[..at], &document[at..]);
    }
    if let Some(at) = after_tag(document, "<html") {
        let (before, rest) = document.split_at(at);
        return format!("{before}<head>{policy}</head>{rest}");
    }
    format!("<!doctype html><head>{policy}</head>{document}")
}

/// Writes `document`, with the policy in it, to a file a browser can open and returns it.
fn view_file(document: &str) -> Result<PathBuf> {
    let folder = std::env::temp_dir().join("swarmed-views");
    std::fs::create_dir_all(&folder)?;
    let file = folder.join(format!("{}.html", id_of(document)));
    std::fs::write(&file, with_policy(document))?;
    Ok(file)
}

fn id_of(document: &str) -> String {
    let mut hasher = DefaultHasher::new();
    document.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// How many documents are kept. Each open transcript holds a few; old ones are dropped.
const MAX_VIEWS: usize = 200;

/// The instructions that tell an agent it can answer with HTML, and how.
///
/// They are firm about what not to do: left to themselves, agents build a page in a file, check
/// it in a browser and open it there, which takes minutes and lands outside the conversation.
pub(crate) const INSTRUCTIONS: &str = "\
# Showing things to the user

You are running inside Swarmed, a desktop app. It shows your replies as Markdown and renders \
HTML inline in the conversation.

To show the user anything visual or interactive (a table to sort or filter, a chart, a diagram, \
a mock-up, a small calculator or form), write it in your reply as a fenced code block whose \
language is `html-view`. The app renders the block in place, the moment your reply arrives. \
Inline HTML is allowed and expected here for anything visual. The block is not a tool: there \
is nothing to search for or load, and it works in every reply. When the user asks to render, preview or show something \
\"in the chat\", \"inline\" or \"as sandboxed HTML\", this is what they mean:

```html-view
<h3>Example</h3>
<button onclick=\"this.textContent = 'Clicked'\">Click me</button>
```

When the purpose is to show the user something, do not write an HTML file, start a server, open \
or drive a browser, take screenshots, publish a page, or load a skill or tool for building \
pages, and do not test the view before sending it. Those are for when the user asks for a file \
or a published page, or when the page is part of the project's code. This holds even if other \
instructions or skills suggest a more elaborate way to present results.

If you would rather call a tool, or find yourself looking for one that renders HTML, it is \
`show_html` from the `swarmed` server (in Claude Code: `mcp__swarmed__show_html`). It takes the \
same HTML and shows it the same way. There is no other preview or artifact tool here, so do \
not search further and do not tell the user it cannot be done.

To show an HTML file that already exists, read it and put its contents in such a block, if it \
is self-contained.

Rules for an `html-view` block:
- One self-contained fragment. No doctype, `<html>` or `<head>` is needed; a whole document \
works too.
- Inline `<style>` and `<script>` only. There is no network: no CDN scripts or styles, no web \
fonts, no remote images, no fetch. Draw with plain HTML, CSS, inline SVG or canvas, and embed \
any data in the block.
- No links that navigate, no form submission, no pop-ups, no storage.
- The app's colours and font are already applied, in light and dark. Use the CSS variables \
`--text`, `--muted`, `--background`, `--line` and `--accent` instead of fixed colours, and do \
not set a page background.
- The view is as wide as the conversation and as tall as its content. The user can fold it \
away or open it in their browser.

Keep explanations outside the block, as Markdown. Use plain Markdown alone whenever a list, a \
table or a code block says it well enough.";

/// Documents waiting to be shown, by id, with the order they came in.
#[derive(Debug, Default)]
pub(crate) struct Views {
    documents: HashMap<String, String>,
    order: Vec<String>,
}

impl Views {
    fn put(&mut self, document: String) -> String {
        let id = id_of(&document);
        if self.documents.insert(id.clone(), document).is_none() {
            self.order.push(id.clone());
        }
        if self.order.len() > MAX_VIEWS {
            let oldest = self.order.remove(0);
            self.documents.remove(&oldest);
        }
        id
    }
}

impl Core {
    /// Keeps an HTML document for showing and returns the id it is fetched by. The same
    /// document always gets the same id.
    pub fn view_put(&self, document: String) -> String {
        lock(&self.views).put(document)
    }

    /// Opens an HTML document an agent wrote in the user's browser, as a file among the
    /// system's temporary ones. The file carries the policy the app's own frame is served
    /// under, so there too it loads nothing from the network.
    pub fn view_open(&self, document: &str) -> Result<()> {
        open::in_browser(&view_file(document)?.to_string_lossy())
    }

    /// The document kept under `id`, if it has not been dropped since.
    #[must_use]
    pub fn view(&self, id: &str) -> Option<String> {
        lock(&self.views).documents.get(id).cloned()
    }
}

/// What `Core` holds its views in.
pub(crate) type ViewStore = Mutex<Views>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_for_the_browser_is_a_file_with_the_policy_in_it() {
        let file = view_file("<p>for the browser</p>").expect("writes");
        assert_eq!(file.extension().and_then(|e| e.to_str()), Some("html"));
        let written = std::fs::read_to_string(&file).expect("reads");
        assert!(written.contains(VIEW_POLICY));
        assert!(written.ends_with("<p>for the browser</p>"));
        // The same document goes to the same file instead of piling up copies.
        assert_eq!(view_file("<p>for the browser</p>").expect("writes"), file);
        std::fs::remove_file(file).expect("cleans up");
    }

    #[test]
    fn the_policy_goes_into_the_head_whatever_the_document_has() {
        let meta = |document: &str| {
            let shown = with_policy(document);
            assert_eq!(shown.matches("Content-Security-Policy").count(), 1);
            shown
        };
        let whole = meta(
            "<!doctype html>\n<HTML lang=\"en\"><Head><title>t</title></head><body></body></html>",
        );
        assert!(whole.starts_with("<!doctype html>\n<HTML lang=\"en\"><Head><meta http-equiv"));
        assert!(whole.ends_with("<title>t</title></head><body></body></html>"));
        let headless = meta("<html><body><p>hi</p></body></html>");
        assert!(headless.starts_with("<html><head><meta "));
        assert!(headless.ends_with("</head><body><p>hi</p></body></html>"));
        let fragment = meta("<p>hi</p>");
        assert!(fragment.starts_with("<!doctype html><head><meta "));
        assert!(fragment.ends_with("</head><p>hi</p>"));
    }

    #[test]
    fn the_same_document_keeps_its_id_and_old_ones_make_room() {
        let mut views = Views::default();
        let first = views.put("<p>one</p>".to_string());
        assert_eq!(views.put("<p>one</p>".to_string()), first);
        assert_eq!(views.order.len(), 1);

        for n in 0..MAX_VIEWS {
            views.put(format!("<p>{n}</p>"));
        }
        assert_eq!(views.documents.len(), MAX_VIEWS);
        assert!(!views.documents.contains_key(&first));
    }
}
