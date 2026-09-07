use {
    grafdag_core::{
        layout::{
            Layout,
            NodeSize,
            PlacedNode,
        },
        Action,
        CoalesceKey,
        Document,
        EdgeId,
        History,
        LayerId,
        NodeId,
    },
    lunk::{
        Animator,
        EventGraph,
        HistPrim,
        Prim,
        ProcessingContext,
    },
    rooting::El,
    std::{
        cell::{
            Cell,
            RefCell,
        },
        collections::HashMap,
        rc::Rc,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub enum SearchTarget {
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Layers,
    EditNode(NodeId),
    EditEdge(EdgeId),
    Search(SearchTarget),
}

/// Cached text measurement for a node.
#[derive(Clone, Debug)]
pub struct Measured {
    pub text: String,
    pub size: NodeSize,
    /// The wrapping width chosen for the text (for hysteresis).
    pub text_width: Option<f64>,
}

/// Overlay buttons drawn next to the focused node on the canvas.
pub struct Overlay {
    pub el: El,
    pub sibling: El,
    pub next: El,
}

#[derive(Default)]
pub struct RenderState {
    pub nodes: HashMap<grafdag_core::layout::PlacementId, super::render::NodeView>,
    pub edges: HashMap<super::render::EdgeKey, super::render::EdgeView>,
}

pub struct State {
    pub eg: EventGraph,
    pub doc: RefCell<Document>,
    pub history: RefCell<History>,
    pub on_change: Box<dyn Fn(&Document)>,
    /// Bumped whenever the document changes.
    pub doc_version: HistPrim<u64>,
    pub sel_start: HistPrim<Option<NodeId>>,
    pub sel_end: HistPrim<Option<NodeId>>,
    /// The selected link between the start and end nodes, if they're linked.
    pub sel_edge: HistPrim<Option<EdgeId>>,
    /// Node under the mouse.
    pub hover: HistPrim<Option<NodeId>>,
    /// Link under the mouse.
    pub hover_edge: HistPrim<Option<EdgeId>>,
    /// Selection history, most recent last.
    pub recent: RefCell<Vec<NodeId>>,
    pub mode: HistPrim<Mode>,
    pub zoom: HistPrim<f64>,
    pub pan: HistPrim<(f64, f64)>,
    pub layout: Prim<Rc<Layout>>,
    /// Set by keyboard commands: scroll the view to the selection after the
    /// next layout.
    pub follow: Cell<bool>,
    pub panel_open: HistPrim<bool>,
    pub status: HistPrim<String>,
    pub measured: RefCell<HashMap<NodeId, Measured>>,
    pub render: RefCell<RenderState>,
    pub overlay: RefCell<Option<Overlay>>,
    /// Canvas size in CSS pixels.
    pub viewport: Cell<(f64, f64)>,
    /// Canvas size (coarsely rounded); its extent along the side axis is the
    /// maximum rank width. Changing it triggers a relayout.
    pub layout_width: HistPrim<u32>,
    pub layout_height: HistPrim<u32>,
    pub can_undo: HistPrim<bool>,
    pub can_redo: HistPrim<bool>,
    /// Elements the side panel wants focused after the next mode change.
    pub focus_request: RefCell<Option<El>>,
    /// Search state
    pub search_query: HistPrim<String>,
    pub search_index: HistPrim<usize>,
    /// Opacity of unselected things.
    pub fade: HistPrim<f64>,
    /// Opacity of unselected things outside the current layer.
    pub fade_secondary: HistPrim<f64>,
    pub animator: Animator,
    /// Whether to animate transitions (false when the browser prefers reduced
    /// motion).
    pub animate: Cell<bool>,
}

pub const DEFAULT_FADE: f64 = 0.6;
pub const DEFAULT_FADE_SECONDARY: f64 = 0.3;

pub fn now_ms() -> f64 {
    return js_sys::Date::now();
}

impl State {
    pub fn new(pc: &mut ProcessingContext, eg: EventGraph, mut doc: Document, on_change: Box<dyn Fn(&Document)>) -> Rc<State> {
        doc.sanitize();
        return Rc::new(State {
            eg: eg.clone(),
            doc: RefCell::new(doc),
            history: RefCell::new(History::default()),
            on_change: on_change,
            doc_version: HistPrim::new(pc, 0),
            sel_start: HistPrim::new(pc, None),
            sel_end: HistPrim::new(pc, None),
            sel_edge: HistPrim::new(pc, None),
            hover: HistPrim::new(pc, None),
            hover_edge: HistPrim::new(pc, None),
            recent: RefCell::new(vec![]),
            mode: HistPrim::new(pc, Mode::Layers),
            zoom: HistPrim::new(pc, 1.),
            pan: HistPrim::new(pc, (40., 40.)),
            layout: Prim::new(Rc::new(Layout::default())),
            follow: Cell::new(false),
            panel_open: HistPrim::new(pc, true),
            status: HistPrim::new(pc, "".to_string()),
            measured: RefCell::new(HashMap::new()),
            render: RefCell::new(RenderState::default()),
            overlay: RefCell::new(None),
            viewport: Cell::new((800., 600.)),
            layout_width: HistPrim::new(pc, 800),
            layout_height: HistPrim::new(pc, 600),
            can_undo: HistPrim::new(pc, false),
            can_redo: HistPrim::new(pc, false),
            focus_request: RefCell::new(None),
            search_query: HistPrim::new(pc, "".to_string()),
            search_index: HistPrim::new(pc, 0),
            fade: HistPrim::new(pc, DEFAULT_FADE),
            fade_secondary: HistPrim::new(pc, DEFAULT_FADE_SECONDARY),
            animator: super::anim::new_animator(&eg),
            animate: Cell::new(!super::anim::prefers_reduced_motion()),
        });
    }

    /// Mark the document as changed (re-layout, notify).
    pub fn bump(&self, pc: &mut ProcessingContext) {
        let v = self.doc_version.get() + 1;
        self.doc_version.set(pc, v);
        let h = self.history.borrow();
        self.can_undo.set(pc, h.can_undo());
        self.can_redo.set(pc, h.can_redo());
    }

    /// Apply actions atomically as one undo level.
    pub fn commit(&self, pc: &mut ProcessingContext, actions: Vec<Action>, key: Option<CoalesceKey>) {
        if actions.is_empty() {
            return;
        }
        {
            let mut doc = self.doc.borrow_mut();
            self.history.borrow_mut().commit(&mut doc, actions, now_ms(), key);
        }
        self.after_change(pc);
    }

    pub fn undo(&self, pc: &mut ProcessingContext) {
        let done = {
            let mut doc = self.doc.borrow_mut();
            self.history.borrow_mut().undo(&mut doc, now_ms())
        };
        if done {
            self.after_change(pc);
        }
    }

    pub fn redo(&self, pc: &mut ProcessingContext) {
        let done = {
            let mut doc = self.doc.borrow_mut();
            self.history.borrow_mut().redo(&mut doc, now_ms())
        };
        if done {
            self.after_change(pc);
        }
    }

    fn after_change(&self, pc: &mut ProcessingContext) {
        {
            let doc = self.doc.borrow();
            // Drop selection of deleted nodes
            if let Some(s) = self.sel_start.get() {
                if doc.node(&s).is_none() {
                    self.sel_start.set(pc, None);
                }
            }
            if let Some(e) = self.sel_end.get() {
                if doc.node(&e).is_none() {
                    self.sel_end.set(pc, None);
                }
            }
            match self.mode.get() {
                Mode::EditNode(n) if doc.node(&n).is_none() => self.mode.set(pc, Mode::Layers),
                Mode::EditEdge(e) if doc.edge(&e).is_none() => self.mode.set(pc, Mode::Layers),
                _ => { },
            }
            (self.on_change)(&doc);
        }
        self.sync_edge(pc);
        self.bump(pc);
    }

    pub fn set_start(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        if let Some(id) = &id {
            self.touch_recent(id);
        }
        self.sel_start.set(pc, id);
        self.sync_edge(pc);
    }

    pub fn set_end(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        if let Some(id) = &id {
            self.touch_recent(id);
        }
        self.sel_end.set(pc, id);
        self.sync_edge(pc);
    }

    /// Select a specific link (its endpoints become start and end).
    pub fn set_edge(&self, pc: &mut ProcessingContext, id: &EdgeId) {
        let Some(edge) = self.doc.borrow().edge(id).cloned() else {
            return;
        };
        self.touch_recent(&edge.source);
        self.touch_recent(&edge.dest);
        self.sel_start.set(pc, Some(edge.source));
        self.sel_end.set(pc, Some(edge.dest));
        self.sel_edge.set(pc, Some(id.clone()));
    }

    /// Keep the selected link consistent with the selected nodes: keep it if it
    /// still joins them, else pick the first link between them.
    pub fn sync_edge(&self, pc: &mut ProcessingContext) {
        let doc = self.doc.borrow();
        let pair = match (self.sel_start.get(), self.sel_end.get()) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };
        let Some((s, e)) = pair else {
            self.sel_edge.set(pc, None);
            return;
        };
        if let Some(cur) = self.sel_edge.get() {
            if doc.edges_between(&s, &e).any(|x| x.id == cur) {
                return;
            }
        }
        let first = doc.edges_between(&s, &e).next().map(|x| x.id.clone());
        self.sel_edge.set(pc, first);
    }

    fn touch_recent(&self, id: &NodeId) {
        let mut r = self.recent.borrow_mut();
        r.retain(|x| x != id);
        r.push(id.clone());
        if r.len() > 200 {
            r.remove(0);
        }
    }

    /// Rank of a node in the recent-selection history (higher = more recent).
    pub fn recency(&self, id: &NodeId) -> usize {
        return self.recent.borrow().iter().position(|x| x == id).map(|p| p + 1).unwrap_or(0);
    }

    /// The node the view/keyboard focuses on: the end node, or the start node.
    pub fn focus_node(&self) -> Option<NodeId> {
        return self.sel_end.get().or(self.sel_start.get());
    }

    pub fn placed(&self, id: &NodeId) -> Option<PlacedNode> {
        return self.layout.borrow().primary(id).cloned();
    }

    pub fn selected_layer(&self) -> Option<LayerId> {
        return self.doc.borrow().selected_layer.clone();
    }

    /// Nodes visible in the current layout, in layout order (first rank
    /// first, then along the side axis).
    pub fn visible_nodes(&self) -> Vec<NodeId> {
        let layout = self.layout.borrow();
        let mut out: Vec<(f64, f64, NodeId)> = layout.nodes.iter().filter(|n| !n.ghost).map(|n| {
            let c = layout.canonical(grafdag_core::layout::pt(n.rect.cx(), n.rect.cy()));
            (c.y, c.x, n.id.node.clone())
        }).collect();
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.partial_cmp(&b.1).unwrap()));
        return out.into_iter().map(|x| x.2).collect();
    }
}
