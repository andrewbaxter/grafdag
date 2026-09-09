use {
    grafdag_core::{
        Action,
        CoalesceKey,
        Document,
        EdgeId,
        History,
        LayerId,
        NodeId,
        layout::{
            Layout,
            NodeSize,
            PlacedNode,
        },
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
        ops::{
            Add,
            Mul,
            Sub,
        },
        rc::Rc,
    },
};

pub const DEFAULT_FADE: f64 = 0.6;
pub const DEFAULT_FADE_SECONDARY: f64 = 0.3;

#[derive(Clone, Debug)]
pub struct Measured {
    pub chrome: f64,
    pub container: bool,
    pub fit: Option<(f64, f64)>,
    pub size: NodeSize,
    pub text: String,
    pub text_width: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    EditEdge(EdgeId),
    EditNode(NodeId),
    Layers,
    Search(SearchTarget),
}

pub fn now_ms() -> f64 {
    return js_sys::Date::now();
}

pub struct Overlay {
    pub el: El,
    pub next: El,
    pub sibling: El,
}

#[derive(Default)]
pub struct RenderState {
    pub edges: HashMap<super::render::EdgeKey, super::render::EdgeView>,
    pub nodes: HashMap<grafdag_core::layout::PlacementId, super::render::NodeView>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchTarget {
    Extend,
    Replace,
}

pub struct State {
    pub animate: Cell<bool>,
    pub animator: Animator,
    pub can_redo: HistPrim<bool>,
    pub can_undo: HistPrim<bool>,
    pub doc: RefCell<Document>,
    pub doc_version: HistPrim<u64>,
    pub eg: EventGraph,
    pub fade: HistPrim<f64>,
    pub fade_secondary: HistPrim<f64>,
    pub focus_request: RefCell<Option<El>>,
    pub follow: Cell<bool>,
    pub history: RefCell<History>,
    pub hover: HistPrim<Option<NodeId>>,
    pub hover_edge: HistPrim<Option<EdgeId>>,
    pub layout: Prim<Rc<Layout>>,
    pub layout_height: HistPrim<u32>,
    pub layout_width: HistPrim<u32>,
    pub measured: RefCell<HashMap<NodeId, Measured>>,
    pub mode: HistPrim<Mode>,
    pub on_change: Box<dyn Fn(&Document)>,
    pub overlay: RefCell<Option<Overlay>>,
    pub pan: HistPrim<Vec2>,
    pub panel_open: HistPrim<bool>,
    pub peek: HistPrim<Option<NodeId>>,
    pub peek_offset: HistPrim<Vec2>,
    pub recent: RefCell<Vec<NodeId>>,
    pub render: RefCell<RenderState>,
    pub search_index: HistPrim<usize>,
    pub search_query: HistPrim<String>,
    pub sel_edge: HistPrim<Option<EdgeId>>,
    pub sel_end: HistPrim<Option<NodeId>>,
    pub sel_start: HistPrim<Option<NodeId>>,
    pub status: HistPrim<String>,
    pub viewport: Cell<(f64, f64)>,
    pub zoom: HistPrim<f64>,
}

impl State {
    fn after_change(&self, pc: &mut ProcessingContext) {
        let (start, end) = {
            let doc = self.doc.borrow();
            let start = self.sel_start.get().filter(|s| doc.node(s).is_some());
            let end = self.sel_end.get().filter(|e| doc.node(e).is_some());
            match self.mode.get() {
                Mode::EditNode(n) if doc.node(&n).is_none() => self.mode.set(pc, Mode::Layers),
                Mode::EditEdge(e) if doc.edge(&e).is_none() => self.mode.set(pc, Mode::Layers),
                _ => { },
            }
            (self.on_change)(&doc);
            (start, end)
        };
        self.set_selection(pc, start, end);
        self.bump(pc);
    }

    pub fn anchor_node(&self) -> Option<NodeId> {
        return self.sel_start.get().or(self.sel_end.get());
    }

    pub fn bump(&self, pc: &mut ProcessingContext) {
        let v = self.doc_version.get() + 1;
        self.doc_version.set(pc, v);
        let h = self.history.borrow();
        self.can_undo.set(pc, h.can_undo());
        self.can_redo.set(pc, h.can_redo());
    }

    pub fn clear_selection(&self, pc: &mut ProcessingContext) {
        self.set_selection(pc, None, None);
    }

    pub fn click_extend(&self, pc: &mut ProcessingContext, id: &NodeId) {
        if self.sel_end.get().as_ref() == Some(id) {
            self.set_end(pc, None);
        } else {
            self.extend_selection(pc, id.clone());
        }
    }

    pub fn click_select(&self, pc: &mut ProcessingContext, id: &NodeId) {
        if self.sel_end.get().as_ref() == Some(id) && self.sel_start.get().is_none() {
            self.clear_selection(pc);
        } else {
            self.select_only(pc, id.clone());
        }
    }

    fn close_stale_editor(&self, pc: &mut ProcessingContext) {
        match self.mode.get() {
            Mode::EditNode(id) => {
                if self.sel_start.get().as_ref() != Some(&id) && self.sel_end.get().as_ref() != Some(&id) {
                    self.mode.set(pc, Mode::Layers);
                }
            },
            Mode::EditEdge(id) => {
                if self.sel_edge.get().as_ref() != Some(&id) {
                    self.mode.set(pc, Mode::Layers);
                }
            },
            _ => { },
        }
    }

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

    pub fn extend_selection(&self, pc: &mut ProcessingContext, id: NodeId) {
        self.set_selection(pc, self.anchor_node(), Some(id));
    }

    pub fn focus_node(&self) -> Option<NodeId> {
        return self.sel_end.get();
    }

    pub fn new(
        pc: &mut ProcessingContext,
        eg: EventGraph,
        mut doc: Document,
        on_change: Box<dyn Fn(&Document)>,
    ) -> Rc<State> {
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
            pan: HistPrim::new(pc, Vec2(40., 40.)),
            peek_offset: HistPrim::new(pc, Vec2::default()),
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
            peek: HistPrim::new(pc, None),
            fade: HistPrim::new(pc, DEFAULT_FADE),
            fade_secondary: HistPrim::new(pc, DEFAULT_FADE_SECONDARY),
            animator: super::anim::new_animator(&eg),
            animate: Cell::new(!super::anim::prefers_reduced_motion()),
        });
    }

    pub fn placed(&self, id: &NodeId) -> Option<PlacedNode> {
        return self.layout.borrow().primary(id).cloned();
    }

    pub fn recency(&self, id: &NodeId) -> usize {
        return self.recent.borrow().iter().position(|x| x == id).map(|p| p + 1).unwrap_or(0);
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

    pub fn select_only(&self, pc: &mut ProcessingContext, id: NodeId) {
        self.set_selection(pc, None, Some(id));
    }

    pub fn selected_layer(&self) -> Option<LayerId> {
        return self.doc.borrow().selected_layer.clone();
    }

    pub fn set_edge(&self, pc: &mut ProcessingContext, id: &EdgeId) {
        let Some(edge) = self.doc.borrow().edge(id).cloned() else {
            return;
        };
        self.set_selection(pc, Some(edge.source), Some(edge.dest));
        self.sel_edge.set(pc, Some(id.clone()));
        self.close_stale_editor(pc);
    }

    pub fn set_end(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        self.set_selection(pc, self.sel_start.get(), id);
    }

    pub fn set_selection(&self, pc: &mut ProcessingContext, start: Option<NodeId>, end: Option<NodeId>) {
        let (start, end) = match (start, end) {
            (Some(s), None) => (None, Some(s)),
            (Some(s), Some(e)) if s == e => (None, Some(e)),
            both => both,
        };
        if let Some(s) = &start {
            self.touch_recent(s);
        }
        if let Some(e) = &end {
            self.touch_recent(e);
        }
        self.sel_start.set(pc, start);
        self.sel_end.set(pc, end);
        self.sync_edge(pc);
    }

    pub fn set_start(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        self.set_selection(pc, id, self.sel_end.get());
    }

    pub fn sync_edge(&self, pc: &mut ProcessingContext) {
        let pair = match (self.sel_start.get(), self.sel_end.get()) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };
        let Some((s, e)) = pair else {
            self.sel_edge.set(pc, None);
            self.close_stale_editor(pc);
            return;
        };
        let keep =
            self
                .sel_edge
                .get()
                .map(|cur| self.edge_touches(&cur, &s) && self.connecting_edges(&s, &e).contains(&cur))
                .unwrap_or(false);
        if !keep {
            self.sel_edge.set(pc, self.connecting_edge_from(&s, &e));
        }
        self.close_stale_editor(pc);
    }

    fn touch_recent(&self, id: &NodeId) {
        let mut r = self.recent.borrow_mut();
        r.retain(|x| x != id);
        r.push(id.clone());
        if r.len() > 200 {
            r.remove(0);
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec2(pub f64, pub f64);

impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, o: Vec2) -> Vec2 {
        return Vec2(self.0 + o.0, self.1 + o.1);
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;

    fn mul(self, k: f64) -> Vec2 {
        return Vec2(self.0 * k, self.1 * k);
    }
}

impl Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, o: Vec2) -> Vec2 {
        return Vec2(self.0 - o.0, self.1 - o.1);
    }
}
