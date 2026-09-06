//! Editor commands, invoked from the keyboard, mouse and toolbar.
use {
    super::state::{
        Mode,
        SearchTarget,
        State,
    },
    grafdag_core::{
        delete_node_actions,
        Action,
        Edge,
        Node,
        NodeId,
    },
    lunk::ProcessingContext,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Left,
    Right,
}

impl State {
    /// Nodes in the same rank of the same island as `id`, left to right.
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

    /// Visible successors of a node, ordered left to right.
    pub fn next_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let mut out: Vec<NodeId> = doc.edges.iter().filter(|e| &e.source == id && doc.edge_visible(e)).map(|e| e.dest.clone()).collect();
        self.sort_by_x(&mut out);
        return out;
    }

    /// Visible predecessors of a node, ordered left to right.
    pub fn prev_nodes(&self, id: &NodeId) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let mut out: Vec<NodeId> = doc.edges.iter().filter(|e| &e.dest == id && doc.edge_visible(e)).map(|e| e.source.clone()).collect();
        self.sort_by_x(&mut out);
        return out;
    }

    fn sort_by_x(&self, ids: &mut Vec<NodeId>) {
        let layout = self.layout.borrow();
        ids.dedup();
        ids.sort_by(|a, b| {
            let xa = layout.primary(a).map(|n| n.rect.x).unwrap_or(0.);
            let xb = layout.primary(b).map(|n| n.rect.x).unwrap_or(0.);
            xa.partial_cmp(&xb).unwrap()
        });
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

    fn ensure_start(&self, pc: &mut ProcessingContext) -> Option<NodeId> {
        if let Some(s) = self.sel_start.get() {
            return Some(s);
        }
        let first = self.visible_nodes().into_iter().next()?;
        self.set_start(pc, Some(first.clone()));
        return Some(first);
    }

    // Selection movement

    pub fn cmd_sibling(&self, pc: &mut ProcessingContext, dir: Dir) {
        self.follow.set(true);
        let Some(start) = self.ensure_start(pc) else {
            return;
        };
        let forward = dir == Dir::Right;
        match self.sel_end.get() {
            None => {
                let sibs = self.siblings(&start);
                let next = Self::cycle(&sibs, Some(&start), forward);
                if next.as_ref() != Some(&start) {
                    self.set_end(pc, next);
                }
            },
            Some(end) => {
                let sibs = self.siblings(&end);
                let next = Self::cycle(&sibs, Some(&end), forward);
                if next.as_ref() != Some(&start) {
                    self.set_end(pc, next);
                } else if sibs.len() > 2 {
                    // Skip over the start node
                    self.set_end(pc, Self::cycle(&sibs, Some(&start), forward));
                }
            },
        }
    }

    /// Whether the end node is drawn below (Some(true)) or above (Some(false))
    /// the start node; None if there's no end node or they're level.
    fn selection_goes_down(&self) -> Option<bool> {
        let (s, e) = self.selected_pair()?;
        let sy = self.placed(&s)?.rect.cy();
        let ey = self.placed(&e)?.rect.cy();
        if (sy - ey).abs() < 1. {
            return None;
        }
        return Some(ey > sy);
    }

    /// Move forward (down). If the selection currently points up, this swaps
    /// start and end instead.
    pub fn cmd_forward(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        if self.selection_goes_down() == Some(false) {
            self.cmd_flip(pc);
            return;
        }
        let Some(start) = self.ensure_start(pc) else {
            return;
        };
        match self.sel_end.get() {
            None => {
                let next = self.next_nodes(&start);
                let pick = self.pick_recent(&next);
                self.set_end(pc, pick);
            },
            Some(end) => {
                let next: Vec<NodeId> = self.next_nodes(&end).into_iter().filter(|n| n != &end).collect();
                let pick = self.pick_recent(&next);
                self.set_start(pc, Some(end));
                self.set_end(pc, pick);
            },
        }
    }

    /// Move backward (up). If the selection currently points down, this swaps
    /// start and end instead.
    pub fn cmd_backward(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        if self.selection_goes_down() == Some(true) {
            self.cmd_flip(pc);
            return;
        }
        let Some(start) = self.ensure_start(pc) else {
            return;
        };
        match self.sel_end.get() {
            None => {
                let prev = self.prev_nodes(&start);
                let pick = self.pick_recent(&prev);
                self.set_end(pc, pick);
            },
            Some(_) => {
                let prev: Vec<NodeId> = self.prev_nodes(&start).into_iter().filter(|n| n != &start).collect();
                let Some(pick) = self.pick_recent(&prev) else {
                    return;
                };
                self.set_end(pc, Some(start));
                self.set_start(pc, Some(pick));
            },
        }
    }

    /// Select the end node from the start node's successors (cycling).
    pub fn cmd_select_next(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let Some(start) = self.ensure_start(pc) else {
            return;
        };
        let next = self.next_nodes(&start);
        let end = self.sel_end.get();
        self.set_end(pc, Self::cycle(&next, end.as_ref(), true));
    }

    /// Select the end node from the start node's predecessors (cycling).
    pub fn cmd_select_prev(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let Some(start) = self.ensure_start(pc) else {
            return;
        };
        let prev = self.prev_nodes(&start);
        let end = self.sel_end.get();
        self.set_end(pc, Self::cycle(&prev, end.as_ref(), true));
    }

    pub fn cmd_flip(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let s = self.sel_start.get();
        let e = self.sel_end.get();
        if e.is_none() {
            return;
        }
        self.set_start(pc, e);
        self.set_end(pc, s);
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
        self.set_start(pc, root);
        self.set_end(pc, None);
    }

    pub fn cmd_escape(&self, pc: &mut ProcessingContext) {
        match self.mode.get() {
            Mode::Layers => {
                if self.sel_end.get().is_some() {
                    self.set_end(pc, None);
                } else {
                    self.set_start(pc, None);
                }
            },
            _ => {
                self.mode.set(pc, Mode::Layers);
            },
        }
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

    pub fn cmd_unlink(&self, pc: &mut ProcessingContext) {
        let Some((s, e)) = self.selected_pair() else {
            return;
        };
        let actions: Vec<Action> = self.doc.borrow().edges_between(&s, &e).map(|x| Action::EdgeDelete(x.id.clone())).collect();
        self.commit(pc, actions, None);
    }

    pub fn cmd_reverse(&self, pc: &mut ProcessingContext) {
        let Some((s, e)) = self.selected_pair() else {
            return;
        };
        let actions: Vec<Action> = self.doc.borrow().edges_between(&s, &e).map(|x| {
            let mut x = x.clone();
            std::mem::swap(&mut x.source, &mut x.dest);
            Action::EdgeModify(x)
        }).collect();
        self.commit(pc, actions, None);
    }

    /// Whether new links from `from` should point towards `from` (true) rather
    /// than away, based on the direction of the edge between the selected
    /// nodes.
    fn inward_direction(&self) -> bool {
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

    /// Create a node linked from the end node (or the start node if no end),
    /// then move the selection forward onto it and edit it.
    pub fn cmd_new_next(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let inward = self.inward_direction();
        let id = match (self.sel_start.get(), self.sel_end.get()) {
            (Some(s), Some(e)) => {
                let id = self.create_linked(pc, Some(&e), &e, inward);
                let _ = s;
                self.set_start(pc, Some(e));
                id
            },
            (Some(s), None) => {
                let id = self.create_linked(pc, Some(&s), &s, false);
                id
            },
            (None, _) => {
                let reference = NodeId("".into());
                let id = self.create_linked(pc, None, &reference, false);
                self.set_start(pc, Some(id.clone()));
                self.set_end(pc, None);
                self.mode.set(pc, Mode::EditNode(id.clone()));
                return;
            },
        };
        self.set_end(pc, Some(id.clone()));
        self.mode.set(pc, Mode::EditNode(id));
    }

    /// Create a node linked from the start node (a sibling of the end node),
    /// select it as the end node and edit it.
    pub fn cmd_new_sibling(&self, pc: &mut ProcessingContext) {
        self.follow.set(true);
        let inward = self.inward_direction();
        let id = match (self.sel_start.get(), self.sel_end.get()) {
            (Some(s), Some(e)) => {
                self.create_linked(pc, Some(&s), &e, inward)
            },
            (Some(s), None) => {
                // Sibling of the start node: linked from its predecessor if any
                let prev = self.prev_nodes(&s);
                let from = self.pick_recent(&prev);
                let id = self.create_linked(pc, from.as_ref(), &s, false);
                if let Some(from) = from {
                    self.set_start(pc, Some(from));
                }
                id
            },
            (None, _) => {
                let id = self.create_linked(pc, None, &NodeId("".into()), false);
                self.set_start(pc, Some(id.clone()));
                self.set_end(pc, None);
                self.mode.set(pc, Mode::EditNode(id.clone()));
                return;
            },
        };
        self.set_end(pc, Some(id.clone()));
        self.mode.set(pc, Mode::EditNode(id));
    }

    pub fn cmd_delete(&self, pc: &mut ProcessingContext) {
        let target = match (self.sel_start.get(), self.sel_end.get()) {
            (_, Some(e)) => e,
            (Some(s), None) => s,
            _ => return,
        };
        let actions = delete_node_actions(&self.doc.borrow(), &target);
        self.commit(pc, actions, None);
    }

    pub fn cmd_delete_node(&self, pc: &mut ProcessingContext, id: &NodeId) {
        let actions = delete_node_actions(&self.doc.borrow(), id);
        self.commit(pc, actions, None);
    }

    /// Edit the end node (or the start node if there's no end node).
    pub fn cmd_edit(&self, pc: &mut ProcessingContext) {
        let Some(target) = self.focus_node() else {
            return;
        };
        self.mode.set(pc, Mode::EditNode(target));
    }

    pub fn cmd_edit_start(&self, pc: &mut ProcessingContext) {
        let Some(target) = self.sel_start.get() else {
            return;
        };
        self.mode.set(pc, Mode::EditNode(target));
    }

    pub fn cmd_edit_link(&self, pc: &mut ProcessingContext) {
        let Some((s, e)) = self.selected_pair() else {
            return;
        };
        let edge = self.doc.borrow().edges_between(&s, &e).next().map(|x| x.id.clone());
        if let Some(edge) = edge {
            self.mode.set(pc, Mode::EditEdge(edge));
        }
    }

    pub fn cmd_search(&self, pc: &mut ProcessingContext, target: SearchTarget) {
        if target == SearchTarget::Start {
            self.set_end(pc, None);
        }
        self.search_query.set(pc, "".to_string());
        self.search_index.set(pc, 0);
        self.mode.set(pc, Mode::Search(target));
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
        self.follow.set(true);
        match target {
            SearchTarget::Start => {
                self.set_start(pc, Some(id.clone()));
                self.set_end(pc, None);
            },
            SearchTarget::End => {
                if self.sel_start.get().is_none() {
                    self.set_start(pc, Some(id.clone()));
                } else {
                    self.set_end(pc, Some(id.clone()));
                }
            },
        }
        self.mode.set(pc, Mode::Layers);
    }

    // View

    pub fn cmd_zoom(&self, pc: &mut ProcessingContext, factor: f64, center: Option<(f64, f64)>) {
        let (vw, vh) = self.viewport.get();
        let (cx, cy) = center.unwrap_or((vw / 2., vh / 2.));
        let z0 = self.zoom.get();
        let z1 = (z0 * factor).clamp(0.05, 8.);
        let (px, py) = self.pan.get();
        let k = z1 / z0;
        self.pan.set(pc, (cx - (cx - px) * k, cy - (cy - py) * k));
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
        self.pan.set(pc, ((vw - layout.width * z) / 2., (vh - layout.height * z) / 2.));
    }

    /// Pan so the focused node is visible (called after layout when `follow`
    /// is set).
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
        let (px, py) = self.pan.get();
        let margin = 30.;
        let left = px + n.rect.x * z;
        let top = py + n.rect.y * z;
        let right = px + n.rect.right() * z;
        let bottom = py + n.rect.bottom() * z;
        let mut npx = px;
        let mut npy = py;
        if left < margin || right > vw - margin {
            if n.rect.w * z > vw - 2. * margin {
                npx = margin - n.rect.x * z;
            } else if left < margin {
                npx = px + (margin - left);
            } else {
                npx = px - (right - (vw - margin));
            }
        }
        if top < margin || bottom > vh - margin {
            if n.rect.h * z > vh - 2. * margin {
                npy = margin - n.rect.y * z;
            } else if top < margin {
                npy = py + (margin - top);
            } else {
                npy = py - (bottom - (vh - margin));
            }
        }
        if npx != px || npy != py {
            self.pan.set(pc, (npx, npy));
        }
    }

    pub fn cmd_toggle_panel(&self, pc: &mut ProcessingContext) {
        let v = !self.panel_open.get();
        self.panel_open.set(pc, v);
    }
}
