//! The graph: every page of the space as a dot, with a line for each link
//! between two pages and a lighter line from a page to the pages inside it.
//! This file gives the page script what to draw (`data-graph`) and the same
//! as words for a screen reader; the drawing, the moving and the zoom are in
//! assets/bridge.js. A click on a dot opens its page.

use crate::state::{page_graph, AppState};
use dioxus::prelude::*;

#[component]
pub fn GraphView() -> Element {
    let state = use_context::<AppState>();
    let graph = use_memo(move || {
        let space = state.ui.read().space.clone();
        page_graph(&state.pages.read(), &state.blocks.read(), &space)
    });
    let g = graph.read();
    let (pages, links) = (g.nodes.len(), g.links());
    let count = format!(
        "{pages} {} · {links} {}",
        if pages == 1 { "page" } else { "pages" },
        if links == 1 { "link" } else { "links" }
    );
    let json = g.to_json();
    rsx! {
        div { class: "graph", id: "graph", "data-graph": "{json}",
            canvas { class: "graph-canvas", role: "img", "aria-label": "Graph of {count}. Each dot is a page; a line joins two pages when one links to the other." }
            input {
                class: "graph-find",
                id: "graph-find",
                r#type: "text",
                placeholder: "Find a page",
                "aria-label": "Find a page in the graph",
                spellcheck: false,
            }
            div { class: "graph-count", "{count}" }
            if pages == 0 {
                div { class: "graph-empty", "No pages yet. Each page you make shows here as a dot." }
            } else if links == 0 {
                div { class: "graph-hint", "Type @ in a page to link it to another. Each link is a line here." }
            }
            // The same as words, for a reader that cannot see the picture.
            ul { class: "graph-words",
                for (i, n) in g.nodes.iter().enumerate() {
                    li { key: "{n.id}",
                        "{n.title}"
                        if !g.joined_to(i).is_empty() {
                            ": joined to {g.joined_to(i).join(\", \")}"
                        }
                    }
                }
            }
        }
    }
}
