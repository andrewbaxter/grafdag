//! The DAG editor widget: a canvas with a toolbar and side panel.
//!
//! Reactivity is via `lunk` (all reactive properties are prims); DOM
//! ownership is via `rooting`. Styling is entirely in CSS; the widget only
//! sets classes, attributes and positions/sizes.
pub mod state;
pub mod commands;
pub mod render;
pub mod interact;
pub mod panel;
pub mod toolbar;

use {
    grafdag_core::Document,
    lunk::EventGraph,
    rooting::{
        el,
        El,
    },
    state::State,
    std::rc::Rc,
};

pub struct Widget {
    root: El,
    state: Rc<State>,
}

impl Widget {
    /// Create the widget. `on_change` is called with the new document after
    /// every modification (use it to persist the document).
    pub fn new(eg: &EventGraph, doc: Document, on_change: Box<dyn Fn(&Document)>) -> Widget {
        let (root, state) = eg.event(|pc| {
            let state = State::new(pc, eg.clone(), doc, on_change);
            let canvas = render::build_canvas(pc, &state);
            let panel = panel::build_panel(pc, &state);
            let toolbar = toolbar::build_toolbar(pc, &state);
            let main = el("div").classes(&["gd_main"]).push(canvas).push(panel);
            let root = el("div").classes(&["gd_app"]).push(toolbar).push(main);
            interact::attach(&root, &state);
            return (root, state);
        }).expect("Widget must be created outside of event processing");
        return Widget {
            root: root,
            state: state,
        };
    }

    pub fn el(&self) -> &El {
        return &self.root;
    }

    /// Measure and lay out the document. Call once after the widget's element
    /// is attached to the document (text can't be measured before that).
    pub fn refresh(&self) {
        let state = self.state.clone();
        self.state.eg.event(|pc| {
            state.bump(pc);
        });
    }

    pub fn state(&self) -> &Rc<State> {
        return &self.state;
    }
}
