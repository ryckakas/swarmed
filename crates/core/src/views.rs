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
pub(crate) const INSTRUCTIONS: &str = "This conversation is shown in an app that can render \
    HTML. When a table to sort or filter, a chart, or another small interactive view would \
    serve the user better than text, write it as a fenced code block with the language \
    `html-view`. It is shown in a sandboxed frame: one self-contained fragment with inline CSS \
    and JavaScript only, no network access and no external files. Its text and background \
    colours are already set to match the app, in light and dark. Keep explanations outside \
    the block, and use plain Markdown whenever that is enough.";

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
