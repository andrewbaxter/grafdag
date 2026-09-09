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
        ops::{
            Add,
            Mul,
            Sub,
        },
        rc::Rc,
    },
};

/// A screen offset as an animatable value.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec2(pub f64, pub f64);

impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, o: Vec2) -> Vec2 {
        return Vec2(self.0 + o.0, self.1 + o.1);
    }
}

impl Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, o: Vec2) -> Vec2 {
        return Vec2(self.0 - o.0, self.1 - o.1);
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;

    fn mul(self, k: f64) -> Vec2 {
        return Vec2(self.0 * k, self.1 * k);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchTarget {
    /// The result becomes the whole selection.
    Replace,
    /// The result becomes the primary node, the current primary the anchor.
    Extend,
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
    /// Whether the text was measured as a container title (they're bolder).
    pub container: bool,
    pub size: NodeSize,
    /// The wrapping width chosen for the text (for hysteresis).
    pub text_width: Option<f64>,
    /// Horizontal padding and border of the node box around the text.
    pub chrome: f64,
    /// Wrapping width for a container title spanning its box, and the box
    /// height that produced (see `fit_container_titles`).
    pub fit: Option<(f64, f64)>,
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
    /// The anchor of a two-node selection (the link's source end). Only ever
    /// set together with `sel_end`; see `set_selection`.
    pub sel_start: HistPrim<Option<NodeId>>,
    /// The primary selected node: whatever is selected at all lives here, so
    /// this is set whenever `sel_start` is.
    pub sel_end: HistPrim<Option<NodeId>>,
    /// The selected link between the anchor and primary nodes, if they're
    /// linked.
    pub sel_edge: HistPrim<Option<EdgeId>>,
    /// Node under the mouse.
    pub hover: HistPrim<Option<NodeId>>,
    /// Link under the mouse.
    pub hover_edge: HistPrim<Option<EdgeId>>,
    /// Selection history, most recent last.
    pub recent: RefCell<Vec<NodeId>>,
    pub mode: HistPrim<Mode>,
    pub zoom: HistPrim<f64>,
    /// The base view position (what panning and following move).
    pub pan: HistPrim<Vec2>,
    /// A second view layer on top of `pan`: while a search result is
    /// previewed the view eases to center it, and snaps back afterwards.
    pub peek_offset: HistPrim<Vec2>,
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
    /// The search result being previewed (the row under the mouse, else the
    /// current row): highlighted like a hovered node, and the view centers on
    /// it.
    pub peek: HistPrim<Option<NodeId>>,
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
        let (start, end) = {
            let doc = self.doc.borrow();
            // Drop selection of deleted nodes
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
        // Deleting the primary node promotes the anchor, keeping the invariant.
        self.set_selection(pc, start, end);
        self.bump(pc);
    }

    /// Set both selection slots at once. This is the only place the selection
    /// changes, and it enforces the invariant: either nothing is selected, or
    /// there's a primary (end) node, optionally preceded by a distinct anchor
    /// (start) node. A selection that would be left with only an anchor
    /// becomes a lone primary instead.
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

    /// Set the anchor (start) node, keeping the primary. Clearing the anchor
    /// of a pair leaves the primary selected; setting one when there's no
    /// primary makes it the primary.
    pub fn set_start(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        self.set_selection(pc, id, self.sel_end.get());
    }

    /// Set the primary (end) node, keeping the anchor. Clearing the primary of
    /// a pair promotes the anchor.
    pub fn set_end(&self, pc: &mut ProcessingContext, id: Option<NodeId>) {
        self.set_selection(pc, self.sel_start.get(), id);
    }

    /// Select one node, with no anchor.
    pub fn select_only(&self, pc: &mut ProcessingContext, id: NodeId) {
        self.set_selection(pc, None, Some(id));
    }

    pub fn clear_selection(&self, pc: &mut ProcessingContext) {
        self.set_selection(pc, None, None);
    }

    /// Make `id` the primary node, keeping the current selection behind it as
    /// the anchor.
    pub fn extend_selection(&self, pc: &mut ProcessingContext, id: NodeId) {
        self.set_selection(pc, self.anchor_node(), Some(id));
    }

    /// Primary click on a node: it becomes the whole selection, or is
    /// deselected if it already is.
    pub fn click_select(&self, pc: &mut ProcessingContext, id: &NodeId) {
        if self.sel_end.get().as_ref() == Some(id) && self.sel_start.get().is_none() {
            self.clear_selection(pc);
        } else {
            self.select_only(pc, id.clone());
        }
    }

    /// Secondary click on a node: it becomes the primary node with the
    /// previous selection as the anchor; clicking the primary node again drops
    /// it, promoting the anchor.
    pub fn click_extend(&self, pc: &mut ProcessingContext, id: &NodeId) {
        if self.sel_end.get().as_ref() == Some(id) {
            self.set_end(pc, None);
        } else {
            self.extend_selection(pc, id.clone());
        }
    }

    /// Select a specific link (its source becomes the anchor, its dest the
    /// primary).
    pub fn set_edge(&self, pc: &mut ProcessingContext, id: &EdgeId) {
        let Some(edge) = self.doc.borrow().edge(id).cloned() else {
            return;
        };
        self.set_selection(pc, Some(edge.source), Some(edge.dest));
        self.sel_edge.set(pc, Some(id.clone()));
        self.close_stale_editor(pc);
    }

    /// Editors close when their subject is deselected. Other modes (search,
    /// layers) are independent of the selection.
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

    /// Keep the selected link consistent with the selected nodes: keep it if
    /// it's still the anchor's link along a walk to the primary node, else
    /// pick that walk's first link out of the anchor.
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

    /// Rank of a node in the recent-selection history (higher = more recent).
    pub fn recency(&self, id: &NodeId) -> usize {
        return self.recent.borrow().iter().position(|x| x == id).map(|p| p + 1).unwrap_or(0);
    }

    /// The primary selected node: what the view follows and what the
    /// single-node commands (edit, delete, enter/exit) act on.
    pub fn focus_node(&self) -> Option<NodeId> {
        return self.sel_end.get();
    }

    /// The node the primary node is measured against: the anchor if there is
    /// one, else the primary itself (used when a command extends a lone
    /// selection into a pair).
    pub fn anchor_node(&self) -> Option<NodeId> {
        return self.sel_start.get().or(self.sel_end.get());
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
