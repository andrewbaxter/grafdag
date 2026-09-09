pub mod anim;
pub mod commands;
pub mod interact;
pub mod panel;
pub mod render;
pub mod state;
pub mod toolbar;

use {
    grafdag_core::Document,
    lunk::EventGraph,
    rooting::{
        El,
        el,
    },
    state::State,
    std::rc::Rc,
};

pub struct Widget {
    root: El,
    state: Rc<State>,
}

impl Widget {
    pub fn el(&self) -> &El {
        return &self.root;
    }

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
