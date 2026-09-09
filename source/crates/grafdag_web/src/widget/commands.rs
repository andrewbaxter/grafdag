//! Editor commands, invoked from the keyboard, mouse and toolbar.
use {
    super::{
        anim::{
            ease,
            TRANSITION_MS,
        },
        state::{
            Mode,
            SearchTarget,
            State,
            Vec2,
        },
    },
    grafdag_core::{
        delete_node_actions,
        layout::{
            pt,
            Motion,
            ScreenDir,
            Side,
        },
        Action,
        Edge,
        EdgeId,
        Node,
        NodeId,
    },
    lunk::{
        HistPrimEaseExt,
        ProcessingContext,
    },
    std::collections::HashSet,
};

/// Direction along the side axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Prev,
    Next,
}

impl State {
    /// Nodes in the same rank of the same island as `id`, along the side axis.
    pub fn siblings(&self, id: &NodeId) -> Vec<NodeId> {
        let layout = self.layout.borrow();
        let Some(n) = layout.primary(id) else {
            return vec![];
        };
        let Some(island) = layout.islands.get(n.nav.island) else {
            return vec![];
        };
        let Some(rank) = island.ranks.get(n.nav.rank) else {
            return vec![];
        };
        return rank.iter().map(|p| p.node.clone()).collect();
    }

    /// Visible links attached to one side of a node, in the order they're
    /// drawn along that side. Links without a drawn port are omitted.
    pub fn edges_on_side(&self, id: &NodeId, side: Side) -> Vec<(grafdag_core::EdgeId, NodeId)> {
        let doc = self.doc.borrow();
        let layout = self.layout.borrow();
        let Some(here) = layout.primary(id).map(|n| n.id.clone()) else {
            return vec![];
        };
        let mut out: Vec<(f64, grafdag_core::EdgeId, NodeId)> = doc.edges.iter().filter(|e| (&e.source == id || &e.dest == id) && doc.edge_visible(e)).filter_map(|e| {
            let port = layout.port(&e.id, &here)?;
            if port.side != side {
                return None;
            }
            let other = if &e.source == id {
                e.dest.clone()
            } else {
                e.source.clone()
            };
            Some((port.along, e.id.clone(), other))
        }).collect();
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        return out.into_iter().map(|(_, e, n)| (e, n)).collect();
    }

    /// Visible successors of a node, ordered along the side axis.
    pub fn next_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let mut out: Vec<NodeId> = doc.edges.iter().filter(|e| &e.source == id && doc.edge_visible(e)).map(|e| e.dest.clone()).collect();
        self.sort_by_x(&mut out);
        return out;
    }

    /// Visible predecessors of a node, ordered along the side axis.
    pub fn prev_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let mut out: Vec<NodeId> = doc.edges.iter().filter(|e| &e.dest == id && doc.edge_visible(e)).map(|e| e.source.clone()).collect();
        self.sort_by_x(&mut out);
        return out;
    }

    /// Every visible link on a walk from one node to the other, following
    /// links in the direction that leads there (either node may be the
    /// upstream one), and the nodes those walks pass through. Empty if neither
    /// node reaches the other.
    pub fn connecting(&self, start: &NodeId, end: &NodeId) -> (Vec<EdgeId>, Vec<NodeId>) {
        let doc = self.doc.borrow();
        let edges: Vec<&Edge> = doc.edges.iter().filter(|e| doc.edge_visible(e)).collect();
        // Nodes reachable from `from` (itself included), walking links forwards
        // or backwards.
        let reach = |from: &NodeId, forward: bool| -> HashSet<NodeId> {
            let mut seen = HashSet::new();
            seen.insert(from.clone());
            let mut queue = vec![from.clone()];
            while let Some(here) = queue.pop() {
                for e in &edges {
                    let (near, far) = if forward {
                        (&e.source, &e.dest)
                    } else {
                        (&e.dest, &e.source)
                    };
                    if near == &here && seen.insert(far.clone()) {
                        queue.push(far.clone());
                    }
                }
            }
            return seen;
        };
        let mut out = vec![];
        let mut nodes: Vec<NodeId> = vec![];
        for (upstream, downstream) in [(start, end), (end, start)] {
            let after = reach(upstream, true);
            if !after.contains(downstream) {
                continue;
            }
            // A link is on a walk between them if the upstream node reaches its
            // source and its dest reaches the downstream node.
            let before = reach(downstream, false);
            for e in &edges {
                if after.contains(&e.source) && before.contains(&e.dest) && !out.contains(&e.id) {
                    out.push(e.id.clone());
                    for n in [&e.source, &e.dest] {
                        if !nodes.contains(n) {
                            nodes.push(n.clone());
                        }
                    }
                }
            }
        }
        return (out, nodes);
    }

    /// Just the links of `connecting`.
    pub fn connecting_edges(&self, start: &NodeId, end: &NodeId) -> Vec<EdgeId> {
        return self.connecting(start, end).0;
    }

    /// The link a two-node selection follows out of the anchor: a direct link
    /// if there is one, else the first link of a walk between them.
    pub fn connecting_edge_from(&self, start: &NodeId, end: &NodeId) -> Option<EdgeId> {
        let direct = {
            let doc = self.doc.borrow();
            let direct = doc.edges_between(start, end).next().map(|e| e.id.clone());
            direct
        };
        if direct.is_some() {
            return direct;
        }
        return self.connecting_edges(start, end).into_iter().find(|e| self.edge_touches(e, start));
    }

    /// Whether a link has `node` as one of its ends.
    pub fn edge_touches(&self, edge: &EdgeId, node: &NodeId) -> bool {
        return self.doc.borrow().edge(edge).map(|e| &e.source == node || &e.dest == node).unwrap_or(false);
    }

    /// Visible successors (`forward`) or predecessors of a node.
    fn step_nodes(&self, id: &NodeId, forward: bool) -> Vec<NodeId> {
        return if forward {
            self.next_nodes(id)
        } else {
            self.prev_nodes(id)
        };
    }

    /// Sort nodes along the side axis.
    fn sort_by_x(&self, ids: &mut Vec<NodeId>) {
        let layout = self.layout.borrow();
        ids.dedup();
        let key = |id: &NodeId| layout.primary(id).map(|n| layout.canonical(pt(n.rect.cx(), n.rect.cy())).x).unwrap_or(0.);
        ids.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap());
    }

    /// Prefer the most recently selected candidate, else the first.
    fn pick_recent(&self, candidates: &[NodeId]) -> Option<NodeId> {
        let mut best: Option<(usize, &NodeId)> = None;
        for c in candidates {
            let r = self.recency(c);
            if best.map(|b| r > b.0).unwrap_or(true) {
                best = Some((r, c));
            }
        }
        return best.map(|b| b.1.clone());
    }

    fn cycle(list: &[NodeId], current: Option<&NodeId>, forward: bool) -> Option<NodeId> {
        if list.is_empty() {
            return None;
        }
        let Some(current) = current else {
            return Some(list[0].clone());
        };
        let Some(i) = list.iter().position(|x| x == current) else {
            return Some(list[0].clone());
        };
        let n = list.len();
        let j = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        return Some(list[j].clone());
    }

    /// The primary node, selecting the first visible node if nothing is
    /// selected yet (movement commands start from somewhere).
    fn ensure_focus(&self, pc: &mut ProcessingContext) -> Option<NodeId> {
        if let Some(e) = self.focus_node() {
            return Some(e);
        }
        let first = self.visible_nodes().into_iter().next()?;
        self.select_only(pc, first.clone());
        return Some(first);
    }

    // Selection movement

    /// An arrow key: its meaning depends on the layout's flow. Along the rank
    /// axis it moves forward/backward (with shift: picks the primary node among
    /// the anchor's successors/predecessors); along the side axis it cycles
    /// siblings.
    pub fn cmd_arrow(&self, pc: &mut ProcessingContext, dir: ScreenDir, shift: bool) {
        let flow = self.doc.borrow().flow;
        match (flow.motion(dir), shift) {
            (Motion::Forward, false) => self.cmd_forward(pc),
            (Motion::Forward, true) => self.cmd_select_next(pc),
            (Motion::Backward, false) => self.cmd_backward(pc),
            (Motion::Backward, true) => self.cmd_select_prev(pc),
            (Motion::SideNext, _) => self.cmd_sibling(pc, Dir::Next),
            (Motion::SidePrev, _) => self.cmd_sibling(pc, Dir::Prev),
        }
    }

    /// Turn the layout's flow direction a quarter turn on screen.
    pub fn cmd_rotate_flow(&self, pc: &mut ProcessingContext, cw: bool) {
        self.follow.set(true);
        let flow = self.doc.borrow().flow;
        let next = if cw {
            flow.cw()
        } else {
            flow.ccw()
        };
        self.commit(pc, vec![Action::SetFlow(next)], None);
    }

    /// Cycle the selected link among the links on the same side of the anchor
    /// node (each link counts, even several to the same node); the primary node
    /// follows. Links on the other side aren't siblings, and without a
    /// selected link there's nothing to cycle.
    pub fn cmd_sibling(&self, pc: &mut ProcessingContext, dir: Dir) {
        self.follow.set(true);
        let (Some(start), Some(cur)) = (self.sel_start.get(), self.sel_edge.get()) else {
            return;
        };
        let side = {
            let layout = self.layout.borrow();
            let Some(port) = layout.primary(&start).and_then(|n| layout.port(&cur, &n.id)) else {
                return;
            };
            port.side
        };
        let edges = self.edges_on_side(&start, side);
        let Some(i) = edges.iter().position(|(id, _)| id == &cur) else {
            return;
        };
        let n = edges.len();
        let j = if dir == Dir::Next {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        let (edge, other) = edges[j].clone();
        self.set_selection(pc, Some(start), Some(other));
        self.sel_edge.set(pc, Some(edge));
    }

    /// Whether the primary node is placed after (Some(true)) or before
    /// (Some(false)) the anchor node along the rank axis; None if there's no
    /// anchor or they're level.
    fn selection_goes_forward(&self) -> Option<bool> {
        let (s, e) = self.selected_pair()?;
        let layout = self.layout.borrow();
        let rank_pos = |id: &NodeId| {
            let n = layout.primary(id)?;
            Some(layout.canonical(pt(n.rect.cx(), n.rect.cy())).y)
        };
        let sy = rank_pos(&s)?;
        let ey = rank_pos(&e)?;
        if (sy - ey).abs() < 1. {
            return None;
        }
        return Some(ey > sy);
    }

    /// Step the primary node along the rank axis, trailing the node it came
    /// from as the anchor. If the selection already points the other way, the
    /// step swaps the two nodes instead. Stepping off the end of the graph
    /// leaves the primary node selected on its own.
    fn step(&self, pc: &mut ProcessingContext, forward: bool) {
        self.follow.set(true);
        if self.selection_goes_forward() == Some(!forward) {
            self.cmd_flip(pc);
            return;
        }
        let Some(focus) = self.ensure_focus(pc) else {
            return;
        };
        let candidates: Vec<NodeId> = self.step_nodes(&focus, forward).into_iter().filter(|n| n != &focus).collect();
        let pick = self.pick_recent(&candidates);
        self.set_selection(pc, Some(focus), pick);
    }

    /// Move forward along the rank axis.
    pub fn cmd_forward(&self, pc: &mut ProcessingContext) {
        self.step(pc, true);
    }

    /// Move backward along the rank axis.
    pub fn cmd_backward(&self, pc: &mut ProcessingContext) {
        self.step(pc, false);
    }

    /// Cycle the primary node among the anchor's successors/predecessors. With
    /// a single node selected that node becomes the anchor, so this extends
    /// the selection into a pair.
    fn cycle_relative(&self, pc: &mut ProcessingContext, forward: bool) {
        self.follow.set(true);
        if self.ensure_focus(pc).is_none() {
            return;
        }
        let Some(anchor) = self.anchor_node() else {
            return;
        };
        let candidates = self.step_nodes(&anchor, forward);
        let end = self.sel_end.get();
        self.set_selection(pc, Some(anchor), Self::cycle(&candidates, end.as_ref(), true));
    }

    /// Select the primary node from the anchor's successors (cycling).
    pub fn cmd_select_next(&self, pc: &mut ProcessingContext) {
        self.cycle_relative(pc, true);
    }

    /// Select the primary node from the anchor's predecessors (cycling).
    pub fn cmd_select_prev(&self, pc: &mut ProcessingContext) {
        self.cycle_relative(pc, false);
    }

    /// Pull the primary node back to the far end of the selected link, so a
    /// selection spanning a walk becomes a single hop before it moves.
    fn collapse_to_hop(&self, pc: &mut ProcessingContext) {
        let (Some(start), Some(edge)) = (self.sel_start.get(), self.sel_edge.get()) else {
            return;
        };
        let other = {
            let doc = self.doc.borrow();
            let Some(e) = doc.edge(&edge) else {
                return;
            };
            if e.source == start {
                e.dest.clone()
            } else if e.dest == start {
                e.source.clone()
            } else {
                return;
            }
        };
        if self.sel_end.get().as_ref() == Some(&other) {
            return;
        }
        self.set_selection(pc, Some(start), Some(other));
        self.sel_edge.set(pc, Some(edge));
    }

    pub fn cmd_flip(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        self.collapse_to_hop(pc);
        let (Some(s), Some(e)) = (self.sel_start.get(), self.sel_end.get()) else {
            return;
        };
        self.set_selection(pc, Some(e), Some(s));
    }

    pub fn cmd_island(&self, pc: &mut ProcessingContext, forward: bool) {
        self.follow.set(true);
        let layout = self.layout.borrow().clone();
        if layout.islands.is_empty() {
            return;
        }
        let current = self.focus_node().and_then(|f| layout.primary(&f).map(|n| n.nav.island));
        let n = layout.islands.len();
        let target = match current {
            None => 0,
            Some(i) => if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            },
        };
        let root = layout.islands[target].ranks.first().and_then(|r| r.first()).map(|p| p.node.clone());
        self.set_selection(pc, None, root);
    }

    /// Escape: close an open pane (editor/search) if any, else exit.
    pub fn cmd_escape(&self, pc: &mut ProcessingContext) {
        match self.mode.get() {
            Mode::Layers => self.cmd_exit(pc),
            _ => {
                self.mode.set(pc, Mode::Layers);
            },
        }
    }

    /// Visible child nodes (nodes drawn inside) of a node, in layout order.
    pub fn child_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        return self.visible_nodes().into_iter().filter(|c| doc.node(c).map(|n| n.parents.contains(id)).unwrap_or(false)).collect();
    }

    /// Visible parent nodes (nodes drawn around) of a node.
    pub fn parent_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let Some(n) = doc.node(id) else {
            return vec![];
        };
        let visible = self.visible_nodes();
        return n.parents.iter().filter(|p| visible.contains(p)).cloned().collect();
    }

    /// Enter: select a child of the primary node (the most recently selected
    /// one, else the first).
    pub fn cmd_enter(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let Some(focus) = self.focus_node() else {
            return;
        };
        let children = self.child_nodes(&focus);
        if let Some(child) = self.pick_recent(&children) {
            self.set_end(pc, Some(child));
        }
    }

    /// Exit: select the parent of the primary node. At the top level (no
    /// parent) this drops the anchor, then the primary node.
    pub fn cmd_exit(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let parent = self.focus_node().and_then(|f| self.pick_recent(&self.parent_nodes(&f)));
        if let Some(parent) = parent {
            self.set_end(pc, Some(parent));
        } else if self.sel_start.get().is_some() {
            self.set_start(pc, None);
        } else {
            self.clear_selection(pc);
        }
    }

    /// Make the node's first layer the current layer if the node isn't in the
    /// current one (used when a node is clicked).
    pub fn activate_node_layer(&self, pc: &mut ProcessingContext, id: &NodeId) {
        let target = {
            let doc = self.doc.borrow();
            let Some(node) = doc.node(id) else {
                return;
            };
            if node.layers.is_empty() || doc.selected_layer.as_ref().map(|l| node.layers.contains(l)).unwrap_or(false) {
                return;
            }
            node.layers[0].clone()
        };
        self.commit(pc, vec![Action::SelectLayer(Some(target))], None);
    }

    // Editing

    fn selected_pair(&self) -> Option<(NodeId, NodeId)> {
        return Some((self.sel_start.get()?, self.sel_end.get()?));
    }

    pub fn cmd_link(&self, pc: &mut ProcessingContext) {
        let Some((s, e)) = self.selected_pair() else {
            return;
        };
        if s == e {
            return;
        }
        let edge = {
            let doc = self.doc.borrow();
            if doc.edges.iter().any(|x| x.source == s && x.dest == e) {
                return;
            }
            Edge {
                id: doc.new_edge_id(),
                text: "".into(),
                source: s,
                dest: e,
                layer: doc.selected_layer.clone(),
            }
        };
        self.commit(pc, vec![Action::EdgeCreate {
            edge: edge,
            index: None,
        }], None);
    }

    /// Delete the selected link.
    pub fn cmd_unlink(&self, pc: &mut ProcessingContext) {
        let Some(edge) = self.sel_edge.get() else {
            return;
        };
        self.commit(pc, vec![Action::EdgeDelete(edge)], None);
    }

    /// Reverse the selected link.
    pub fn cmd_reverse(&self, pc: &mut ProcessingContext) {
        let Some(edge) = self.sel_edge.get() else {
            return;
        };
        let Some(mut x) = self.doc.borrow().edge(&edge).cloned() else {
            return;
        };
        std::mem::swap(&mut x.source, &mut x.dest);
        self.commit(pc, vec![Action::EdgeModify(x)], None);
    }

    /// Whether new links from `from` should point towards `from` (true) rather
    /// than away, based on the direction of the edge between the selected
    /// nodes.
    pub fn inward_direction(&self) -> bool {
        let Some((s, e)) = self.selected_pair() else {
            return false;
        };
        let doc = self.doc.borrow();
        let forward = doc.edges.iter().any(|x| x.source == s && x.dest == e);
        let backward = doc.edges.iter().any(|x| x.source == e && x.dest == s);
        return backward && !forward;
    }

    fn create_linked(&self, pc: &mut ProcessingContext, from: Option<&NodeId>, reference: &NodeId, inward: bool) -> NodeId {
        let (node, edge) = {
            let doc = self.doc.borrow();
            let reference_node = doc.node(reference);
            let id = doc.new_node_id();
            let mut layers = vec![];
            if let Some(l) = &doc.selected_layer {
                layers.push(l.clone());
            }
            let node = Node {
                id: id.clone(),
                text: "".into(),
                layers: layers,
                parents: reference_node.map(|n| n.parents.clone()).unwrap_or_default(),
            };
            let edge = from.map(|from| {
                let (source, dest) = if inward {
                    (id.clone(), from.clone())
                } else {
                    (from.clone(), id.clone())
                };
                Edge {
                    id: doc.new_edge_id(),
                    text: "".into(),
                    source: source,
                    dest: dest,
                    layer: doc.selected_layer.clone(),
                }
            });
            (node, edge)
        };
        let id = node.id.clone();
        let mut actions = vec![Action::NodeCreate {
            node: node,
            index: None,
        }];
        if let Some(edge) = edge {
            actions.push(Action::EdgeCreate {
                edge: edge,
                index: None,
            });
        }
        self.commit(pc, actions, None);
        return id;
    }

    /// Create a node, select it as the primary node with `anchor` behind it,
    /// and open its editor.
    fn create_selected(&self, pc: &mut ProcessingContext, from: Option<NodeId>, reference: &NodeId, inward: bool) {
        let id = self.create_linked(pc, from.as_ref(), reference, inward);
        self.set_selection(pc, from, Some(id.clone()));
        self.mode.set(pc, Mode::EditNode(id));
    }

    /// Create a node linked from the primary node, then move the selection
    /// forward onto it and edit it.
    pub fn cmd_new_next(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let inward = self.inward_direction();
        match self.focus_node() {
            Some(focus) => self.create_selected(pc, Some(focus.clone()), &focus, inward),
            None => self.create_selected(pc, None, &NodeId("".into()), false),
        }
    }

    /// The node a new sibling of the primary node would be linked from: the
    /// anchor if there is one, else the primary node's predecessor.
    fn sibling_source(&self) -> Option<NodeId> {
        if let Some(s) = self.sel_start.get() {
            return Some(s);
        }
        return self.pick_recent(&self.prev_nodes(&self.focus_node()?));
    }

    /// Whether a new sibling would be linked to something (vs. becoming a new
    /// island).
    pub fn sibling_possible(&self) -> bool {
        return self.sibling_source().is_some();
    }

    /// Create an unlinked node (a new island), select it and edit it.
    pub fn cmd_new_island(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        self.create_selected(pc, None, &NodeId("".into()), false);
    }

    /// Create a sibling of the primary node (linked from the same node it
    /// hangs off), select it as the primary node and edit it.
    pub fn cmd_new_sibling(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let inward = self.inward_direction();
        let from = self.sibling_source();
        let reference = self.focus_node().unwrap_or_else(|| NodeId("".into()));
        self.create_selected(pc, from, &reference, inward);
    }

    pub fn cmd_delete(&self, pc: &mut ProcessingContext) {
        let Some(target) = self.focus_node() else {
            return;
        };
        let actions = delete_node_actions(&self.doc.borrow(), &target);
        self.commit(pc, actions, None);
    }

    pub fn cmd_delete_node(&self, pc: &mut ProcessingContext, id: &NodeId) {
        let actions = delete_node_actions(&self.doc.borrow(), id);
        self.commit(pc, actions, None);
    }

    /// Edit the primary node.
    pub fn cmd_edit(&self, pc: &mut ProcessingContext) {
        let Some(target) = self.focus_node() else {
            return;
        };
        self.mode.set(pc, Mode::EditNode(target));
    }

    /// Edit the anchor node.
    pub fn cmd_edit_start(&self, pc: &mut ProcessingContext) {
        let Some(target) = self.sel_start.get() else {
            return;
        };
        self.mode.set(pc, Mode::EditNode(target));
    }

    pub fn cmd_edit_link(&self, pc: &mut ProcessingContext) {
        if let Some(edge) = self.sel_edge.get() {
            self.mode.set(pc, Mode::EditEdge(edge));
        }
    }

    pub fn cmd_search(&self, pc: &mut ProcessingContext, target: SearchTarget) {
        self.search_query.set(pc, "".to_string());
        self.search_index.set(pc, 0);
        self.mode.set(pc, Mode::Search(target));
    }

    /// The current result (the preview falls back to this when the mouse
    /// leaves a row).
    pub fn search_current(&self) -> Option<NodeId> {
        return self.search_results().get(self.search_index.get()).map(|(id, _)| id.clone());
    }

    /// The search query changed: results restart from the top.
    pub fn cmd_search_query(&self, pc: &mut ProcessingContext, query: String) {
        self.search_query.set(pc, query);
        self.search_index.set(pc, 0);
    }

    /// The mouse entered or left a result row.
    pub fn cmd_search_hover(&self, pc: &mut ProcessingContext, id: &NodeId, entering: bool) {
        if entering {
            self.peek.set(pc, Some(id.clone()));
        } else if self.peek.get().as_ref() == Some(id) {
            self.peek.set(pc, self.search_current());
        }
    }

    /// Screen offset from the base view that centers a node.
    pub fn centering_offset(&self, id: &NodeId) -> Option<Vec2> {
        let n = self.placed(id)?;
        let (vw, vh) = self.viewport.get();
        let z = self.zoom.get();
        let Vec2(px, py) = self.pan.get();
        let cx = n.rect.cx();
        let cy = n.rect.cy();
        return Some(Vec2(vw / 2. - (px + cx * z), vh / 2. - (py + cy * z)));
    }

    /// Make the previewed view position the base view position, so accepting
    /// a previewed result doesn't move the view.
    fn fold_peek(&self, pc: &mut ProcessingContext) {
        let off = self.peek_offset.get();
        if off == Vec2::default() {
            return;
        }
        self.animator.cancel(&self.peek_offset);
        let p = self.pan.get();
        self.set_pan(pc, p + off);
        self.peek_offset.set(pc, Vec2::default());
    }

    /// Search results for the current query, in layout order.
    pub fn search_results(&self) -> Vec<(NodeId, String)> {
        let query = self.search_query.get().to_lowercase();
        let doc = self.doc.borrow();
        let mut out = vec![];
        for id in self.visible_nodes() {
            let Some(n) = doc.node(&id) else {
                continue;
            };
            if query.is_empty() || n.text.to_lowercase().contains(&query) || n.id.0.to_lowercase().contains(&query) {
                out.push((id, n.text.clone()));
            }
        }
        return out;
    }

    pub fn cmd_search_move(&self, pc: &mut ProcessingContext, delta: i64) {
        let n = self.search_results().len() as i64;
        if n == 0 {
            return;
        }
        let i = (self.search_index.get() as i64 + delta).rem_euclid(n);
        self.search_index.set(pc, i as usize);
    }

    pub fn cmd_search_accept(&self, pc: &mut ProcessingContext) {
        let Mode::Search(target) = self.mode.get() else {
            return;
        };
        let results = self.search_results();
        let Some((id, _)) = results.get(self.search_index.get()) else {
            return;
        };
        self.fold_peek(pc);
        self.follow.set(true);
        match target {
            SearchTarget::Replace => self.select_only(pc, id.clone()),
            SearchTarget::Extend => self.extend_selection(pc, id.clone()),
        }
        self.mode.set(pc, Mode::Layers);
    }

    // View

    pub fn cmd_zoom(&self, pc: &mut ProcessingContext, factor: f64, center: Option<(f64, f64)>) {
        let (vw, vh) = self.viewport.get();
        let (cx, cy) = center.unwrap_or((vw / 2., vh / 2.));
        let z0 = self.zoom.get();
        let z1 = (z0 * factor).clamp(0.05, 8.);
        let Vec2(px, py) = self.pan.get();
        let k = z1 / z0;
        self.set_pan(pc, Vec2(cx - (cx - px) * k, cy - (cy - py) * k));
        self.zoom.set(pc, z1);
    }

    pub fn cmd_fit(&self, pc: &mut ProcessingContext) {
        let layout = self.layout.borrow().clone();
        let (vw, vh) = self.viewport.get();
        if layout.width <= 0. || layout.height <= 0. {
            return;
        }
        let margin = 40.;
        let z = ((vw - 2. * margin) / layout.width).min((vh - 2. * margin) / layout.height).clamp(0.05, 2.);
        self.zoom.set(pc, z);
        self.set_pan(pc, Vec2((vw - layout.width * z) / 2., (vh - layout.height * z) / 2.));
    }

    /// Set the view position directly, stopping any easing in progress.
    pub fn set_pan(&self, pc: &mut ProcessingContext, v: Vec2) {
        self.animator.cancel(&self.pan);
        self.pan.set(pc, v);
    }

    /// Pan so the primary node is visible (called after layout when `follow`
    /// is set): if any part of it is outside the viewport (less a margin),
    /// ease the view to center it. A minimal nudge would leave it hugging the
    /// edge, with whatever comes next in that direction still hidden.
    pub fn follow_selection(&self, pc: &mut ProcessingContext) {
        if !self.follow.replace(false) {
            return;
        }
        let Some(id) = self.focus_node() else {
            return;
        };
        let Some(n) = self.placed(&id) else {
            return;
        };
        let (vw, vh) = self.viewport.get();
        let z = self.zoom.get();
        let Vec2(px, py) = self.pan.get();
        let margin = 30.;
        let left = px + n.rect.x * z;
        let top = py + n.rect.y * z;
        let right = px + n.rect.right() * z;
        let bottom = py + n.rect.bottom() * z;
        if left >= margin && top >= margin && right <= vw - margin && bottom <= vh - margin {
            return;
        }
        let target = Vec2(vw / 2. - n.rect.cx() * z, vh / 2. - n.rect.cy() * z);
        if self.animate.get() {
            self.pan.set_ease(&self.animator, target, TRANSITION_MS, ease);
        } else {
            self.set_pan(pc, target);
        }
    }

    pub fn cmd_toggle_panel(&self, pc: &mut ProcessingContext) {
        let v = !self.panel_open.get();
        self.panel_open.set(pc, v);
    }
}
