//! HTML an agent wrote for showing, held until the window's frame fetches it.
//!
//! A front end hands a finished document over and gets an id back; the shell serves the
//! document from an origin of its own, away from Swarmed's commands. Nothing here is stored:
//! a document can always be handed over again from the transcript it came from.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use crate::{Core, lock};

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
This is the way to show something here. It is not a tool: there is nothing to search for or \
load, and it works in every reply. When the user asks to render, preview or show something \
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
- The view is as wide as the conversation and as tall as its content, up to about 700 pixels.

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
        let mut hasher = DefaultHasher::new();
        document.hash(&mut hasher);
        let id = format!("{:016x}", hasher.finish());
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
