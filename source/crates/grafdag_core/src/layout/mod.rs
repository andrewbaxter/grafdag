pub mod order;
pub mod position;
pub mod rank;
#[cfg(test)]
mod tests;

use {
    crate::document::{
        Document,
        EdgeId,
        LayerId,
        NodeId,
    },
    serde::{
        Deserialize,
        Serialize,
    },
    std::collections::{
        HashMap,
        HashSet,
    },
};

#[derive(Clone, Debug)]
pub(crate) struct ContainerResult {
    pub exit_ports: HashMap<usize, (Side, f64)>,
    pub inset: f64,
    pub islands: Vec<Vec<Vec<usize>>>,
    pub members: Vec<(usize, Rect, NavInfo)>,
    pub size: NodeSize,
    pub title_height: f64,
}

pub(crate) struct Ctx<'a> {
    pub config: &'a LayoutConfig,
    pub instances: Vec<EdgeInstance>,
    pub paths: Vec<EdgePath>,
    pub placements: Vec<Placement>,
    pub prev_x: HashMap<PlacementId, f64>,
    pub prev_y: HashMap<PlacementId, f64>,
    pub results: HashMap<Option<usize>, ContainerResult>,
}

impl<'a> Ctx<'a> {
    pub fn ancestor_in(&self, mut pl: usize, container: Option<usize>) -> Option<usize> {
        loop {
            let parent = self.placements[pl].parent;
            if parent == container {
                return Some(pl);
            }
            pl = parent?;
        }
    }

    pub fn is_container(&self, pl: usize) -> bool {
        return self.placements.iter().any(|p| p.parent == Some(pl));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EdgeInstance {
    pub dest: usize,
    pub edge: EdgeId,
    pub source: usize,
    pub weight: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EdgePath {
    pub dest_stubs: Vec<PathPiece>,
    pub main: Option<PathPiece>,
    pub reversed: bool,
    pub source_stubs: Vec<PathPiece>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExitInfo {
    pub inst: usize,
    pub member: usize,
    pub outgoing: bool,
    pub side: Side,
    pub to_self: bool,
}

#[cfg_attr(feature = "schemask", derive(schemask_derive::Maskoidy))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Flow {
    #[default]
    Down,
    Left,
    Right,
    Up,
}

impl Flow {
    pub const ALL: [Flow; 4] = [Flow::Down, Flow::Right, Flow::Up, Flow::Left];

    pub fn canonical_size(self, s: NodeSize) -> NodeSize {
        if self.horizontal() {
            return NodeSize {
                width: s.height,
                height: s.width,
            };
        }
        return s;
    }

    pub fn ccw(self) -> Flow {
        let i = Flow::ALL.iter().position(|f| *f == self).unwrap();
        return Flow::ALL[(i + 1) % Flow::ALL.len()];
    }

    pub fn cw(self) -> Flow {
        let i = Flow::ALL.iter().position(|f| *f == self).unwrap();
        return Flow::ALL[(i + Flow::ALL.len() - 1) % Flow::ALL.len()];
    }

    pub fn horizontal(self) -> bool {
        return matches!(self, Flow::Right | Flow::Left);
    }

    pub fn motion(self, dir: ScreenDir) -> Motion {
        let forward = self.screen_dir(Motion::Forward);
        let backward = self.screen_dir(Motion::Backward);
        let next = self.screen_dir(Motion::SideNext);
        if dir == forward {
            return Motion::Forward;
        } else if dir == backward {
            return Motion::Backward;
        } else if dir == next {
            return Motion::SideNext;
        } else {
            return Motion::SidePrev;
        }
    }

    pub fn rect_to_screen(self, extent: NodeSize, r: Rect) -> Rect {
        let a = self.to_screen(extent, pt(r.x, r.y));
        let b = self.to_screen(extent, pt(r.right(), r.bottom()));
        return Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x - b.x).abs(),
            h: (a.y - b.y).abs(),
        };
    }

    pub fn screen_dir(self, m: Motion) -> ScreenDir {
        return match (self, m) {
            (Flow::Down, Motion::Forward) | (Flow::Up, Motion::Backward) => ScreenDir::Down,
            (Flow::Down, Motion::Backward) | (Flow::Up, Motion::Forward) => ScreenDir::Up,
            (Flow::Down | Flow::Up, Motion::SideNext) => ScreenDir::Right,
            (Flow::Down | Flow::Up, Motion::SidePrev) => ScreenDir::Left,
            (Flow::Right, Motion::Forward) | (Flow::Left, Motion::Backward) => ScreenDir::Right,
            (Flow::Right, Motion::Backward) | (Flow::Left, Motion::Forward) => ScreenDir::Left,
            (Flow::Right | Flow::Left, Motion::SideNext) => ScreenDir::Down,
            (Flow::Right | Flow::Left, Motion::SidePrev) => ScreenDir::Up,
        };
    }

    pub fn screen_side(self, side: Side) -> ScreenDir {
        return self.screen_dir(match side {
            Side::Before => Motion::Backward,
            Side::After => Motion::Forward,
        });
    }

    pub(crate) fn self_exit_side(self) -> Side {
        return match self.title_at() {
            TitleAt::RankEnd => Side::After,
            _ => Side::Before,
        };
    }

    pub(crate) fn title_at(self) -> TitleAt {
        return match self {
            Flow::Down => TitleAt::RankStart,
            Flow::Up => TitleAt::RankEnd,
            Flow::Right | Flow::Left => TitleAt::SideStart,
        };
    }

    pub fn to_canonical(self, extent: NodeSize, p: Pt) -> Pt {
        return match self {
            Flow::Down => pt(p.x, p.y),
            Flow::Up => pt(p.x, extent.height - p.y),
            Flow::Right => pt(p.y, p.x),
            Flow::Left => pt(p.y, extent.height - p.x),
        };
    }

    pub fn to_screen(self, extent: NodeSize, p: Pt) -> Pt {
        return match self {
            Flow::Down => pt(p.x, p.y),
            Flow::Up => pt(p.x, extent.height - p.y),
            Flow::Right => pt(p.y, p.x),
            Flow::Left => pt(extent.height - p.y, p.x),
        };
    }
}

#[derive(Clone, Debug)]
pub struct Island {
    pub container: Option<NodeId>,
    pub ranks: Vec<Vec<PlacementId>>,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub edges: Vec<PlacedEdge>,
    pub flow: Flow,
    pub height: f64,
    pub islands: Vec<Island>,
    pub nodes: Vec<PlacedNode>,
    pub width: f64,
}

impl Layout {
    pub fn canonical(&self, p: Pt) -> Pt {
        return self.flow.to_canonical(self.extent(), p);
    }

    pub fn extent(&self) -> NodeSize {
        return self.flow.canonical_size(NodeSize {
            width: self.width,
            height: self.height,
        });
    }

    pub fn node(&self, id: &PlacementId) -> Option<&PlacedNode> {
        return self.nodes.iter().find(|n| &n.id == id);
    }

    pub fn port(&self, edge: &EdgeId, at: &PlacementId) -> Option<Port> {
        let node = self.node(at)?;
        let placed = self.edges.iter().find(|e| &e.id == edge && (&e.source == at || &e.dest == at))?;
        let p = self.canonical(if &placed.source == at {
            *placed.points.first()?
        } else {
            *placed.points.last()?
        });
        let center = self.canonical(pt(node.rect.cx(), node.rect.cy()));
        let side = if p.y <= center.y {
            Side::Before
        } else {
            Side::After
        };
        return Some(Port {
            side: side,
            along: p.x,
        });
    }

    pub fn primary(&self, id: &NodeId) -> Option<&PlacedNode> {
        return self.nodes.iter().find(|n| &n.id.node == id && !n.ghost);
    }
}

pub fn layout(
    doc: &Document,
    sizes: &HashMap<NodeId, NodeSize>,
    titles: &HashMap<NodeId, NodeSize>,
    config: &LayoutConfig,
    previous: Option<&Layout>,
) -> Layout {
    let flow = config.flow;
    let default_size = flow.canonical_size(NodeSize {
        width: 60.,
        height: 24.,
    });
    let visible: Vec<&NodeId> = doc.nodes.iter().filter(|n| doc.node_visible(n)).map(|n| &n.id).collect();
    let visible_set: HashSet<&NodeId> = visible.iter().cloned().collect();
    let mut primary_parent: HashMap<NodeId, Option<NodeId>> = HashMap::new();
    for id in &visible {
        let node = doc.node(id).unwrap();
        let parent = node.parents.iter().find(|p| *p != *id && visible_set.contains(p)).cloned();
        primary_parent.insert((*id).clone(), parent);
    }
    for id in &visible {
        let mut seen = HashSet::new();
        let mut at = (*id).clone();
        seen.insert(at.clone());
        loop {
            let Some(Some(p)) = primary_parent.get(&at).cloned() else {
                break;
            };
            if !seen.insert(p.clone()) {
                primary_parent.insert((*id).clone(), None);
                break;
            }
            at = p;
        }
    }
    let container_nodes: HashSet<&NodeId> = primary_parent.values().flatten().collect();
    let mut placements = vec![];
    let mut primary_index: HashMap<NodeId, usize> = HashMap::new();
    for id in &visible {
        let size =
            container_nodes
                .contains(*id)
                .then(|| titles.get(*id))
                .flatten()
                .or_else(|| sizes.get(*id))
                .map(|s| flow.canonical_size(*s))
                .unwrap_or(default_size);
        primary_index.insert((*id).clone(), placements.len());
        placements.push(Placement {
            id: PlacementId {
                node: (*id).clone(),
                container: primary_parent.get(*id).cloned().flatten(),
            },
            parent: None,
            ghost: false,
            size: size,
        });
    }
    for id in &visible {
        let node = doc.node(id).unwrap();
        let primary = primary_parent.get(*id).cloned().flatten();
        let mut seen = HashSet::new();
        for p in &node.parents {
            if p == *id || !visible_set.contains(p) || Some(p) == primary.as_ref() || !seen.insert(p.clone()) {
                continue;
            }
            placements.push(Placement {
                id: PlacementId {
                    node: (*id).clone(),
                    container: Some(p.clone()),
                },
                parent: Some(primary_index[p]),
                ghost: true,
                size: sizes.get(*id).map(|s| flow.canonical_size(*s)).unwrap_or(default_size),
            });
        }
    }
    for i in 0 .. visible.len() {
        let parent = placements[i].id.container.as_ref().map(|p| primary_index[p]);
        placements[i].parent = parent;
    }
    let mut node_layers: HashMap<NodeId, HashSet<LayerId>> = HashMap::new();
    for n in &doc.nodes {
        node_layers.insert(n.id.clone(), n.layers.iter().cloned().collect());
    }
    let selected_layer = doc.selected_layer.clone().filter(|l| doc.layer_active(l));
    let mut instances = vec![];
    for e in &doc.edges {
        if !doc.edge_visible(e) {
            continue;
        }
        let s = primary_index[&e.source];
        let d = primary_index[&e.dest];
        let mut weight = 1.;
        if let Some(sel) = &selected_layer {
            let touches =
                e.layer.as_ref() == Some(sel) || node_layers[&e.source].contains(sel) ||
                    node_layers[&e.dest].contains(sel);
            if touches {
                weight = config.selected_weight;
            }
        }
        instances.push(EdgeInstance {
            edge: e.id.clone(),
            source: s,
            dest: d,
            weight: weight,
        });
        for (i, p) in placements.iter().enumerate() {
            if !p.ghost {
                continue;
            }
            if p.id.node == e.source {
                instances.push(EdgeInstance {
                    edge: e.id.clone(),
                    source: i,
                    dest: d,
                    weight: weight,
                });
            }
            if p.id.node == e.dest {
                instances.push(EdgeInstance {
                    edge: e.id.clone(),
                    source: s,
                    dest: i,
                    weight: weight,
                });
            }
        }
    }
    let mut prev_x = HashMap::new();
    let mut prev_y = HashMap::new();
    if let Some(prev) = previous {
        for n in &prev.nodes {
            let c = prev.canonical(pt(n.rect.cx(), n.rect.cy()));
            prev_x.insert(n.id.clone(), c.x);
            prev_y.insert(n.id.clone(), c.y);
        }
    }
    let n_inst = instances.len();
    let mut ctx = Ctx {
        config: config,
        placements: placements,
        instances: instances,
        prev_x: prev_x,
        prev_y: prev_y,
        paths: vec![
            EdgePath::default();
            n_inst
        ],
        results: HashMap::new(),
    };
    let root = layout_container(&mut ctx, None, vec![]);
    ctx.results.insert(None, root);
    let mut out = Layout::default();
    let root = &ctx.results[&None];
    let flow = ctx.config.flow;
    let extent = root.size;
    let screen = flow.canonical_size(extent);
    out.width = screen.width;
    out.height = screen.height;
    out.flow = flow;
    let mut island_index: HashMap<(Option<usize>, usize), usize> = HashMap::new();
    let mut origins: HashMap<Option<usize>, Pt> = HashMap::new();
    origins.insert(None, pt(0., 0.));
    let mut stack = vec![(None, 0usize)];
    let mut visit_order = vec![];
    while let Some((container, depth)) = stack.pop() {
        visit_order.push((container, depth));
        let res = &ctx.results[&container];
        let origin = origins[&container];
        for (island_i, _) in res.islands.iter().enumerate() {
            island_index.insert((container, island_i), out.islands.len());
            out.islands.push(Island {
                container: container.map(|c| ctx.placements[c].id.node.clone()),
                ranks: res.islands[island_i]
                    .iter()
                    .map(|r| r.iter().map(|m| ctx.placements[*m].id.clone()).collect())
                    .collect(),
            });
        }
        let mut children = vec![];
        for (m, rect, _) in &res.members {
            if let Some(child) = ctx.results.get(&Some(*m)) {
                origins.insert(Some(*m), pt(origin.x + rect.x + child.inset, origin.y + rect.y));
                children.push((Some(*m), depth + 1));
            }
        }
        children.reverse();
        stack.extend(children);
    }
    for (container, depth) in &visit_order {
        let res = &ctx.results[container];
        let origin = origins[container];
        for (m, rect, nav) in &res.members {
            let is_container = ctx.results.contains_key(&Some(*m));
            let title_height = if is_container {
                ctx.results[&Some(*m)].title_height
            } else {
                rect.h
            };
            out.nodes.push(PlacedNode {
                id: ctx.placements[*m].id.clone(),
                rect: flow.rect_to_screen(extent, Rect {
                    x: origin.x + rect.x,
                    y: origin.y + rect.y,
                    w: rect.w,
                    h: rect.h,
                }),
                title_height: title_height,
                ghost: ctx.placements[*m].ghost,
                container: is_container,
                depth: *depth,
                nav: NavInfo {
                    island: island_index[&(*container, nav.island)],
                    rank: nav.rank,
                    index: nav.index,
                },
            });
        }
    }
    for (i, path) in ctx.paths.iter().enumerate() {
        let inst = &ctx.instances[i];
        let Some(main) = &path.main else {
            continue;
        };
        let mut points: Vec<Pt> = vec![];
        let push_piece = |points: &mut Vec<Pt>, piece: &PathPiece, reverse: bool| {
            let Some(origin) = origins.get(&piece.container) else {
                return;
            };
            let mut pts: Vec<Pt> = piece.points.iter().map(|p| pt(origin.x + p.x, origin.y + p.y)).collect();
            if reverse {
                pts.reverse();
            }
            for p in pts {
                if points
                    .last()
                    .map(|l: &Pt| (l.x - p.x).abs() < 0.01 && (l.y - p.y).abs() < 0.01)
                    .unwrap_or(false) {
                    continue;
                }
                points.push(p);
            }
        };
        for piece in &path.source_stubs {
            push_piece(&mut points, piece, false);
        }
        push_piece(&mut points, main, false);
        for piece in path.dest_stubs.iter().rev() {
            push_piece(&mut points, piece, true);
        }
        let points: Vec<Pt> = points.into_iter().map(|p| flow.to_screen(extent, p)).collect();
        let points = {
            let mut out: Vec<Pt> = vec![];
            for p in points {
                if out.len() >= 2 {
                    let a = out[out.len() - 2];
                    let b = out[out.len() - 1];
                    let vertical = (a.x - b.x).abs() < 0.01 && (b.x - p.x).abs() < 0.01;
                    let horizontal = (a.y - b.y).abs() < 0.01 && (b.y - p.y).abs() < 0.01;
                    if vertical || horizontal {
                        out.pop();
                    }
                }
                out.push(p);
            }
            out
        };
        let label = {
            let mut best = None;
            let mut best_score = f64::MIN;
            for w in points.windows(2) {
                let (a, b) = (w[0], w[1]);
                let len = (a.x - b.x).abs() + (a.y - b.y).abs();
                let horizontal = (a.y - b.y).abs() < 0.01;
                let score = len + if horizontal {
                    1000.
                } else {
                    0.
                };
                if score > best_score {
                    best_score = score;
                    best = Some(pt((a.x + b.x) / 2., (a.y + b.y) / 2.));
                }
            }
            best.unwrap_or(points.first().cloned().unwrap_or(pt(0., 0.)))
        };
        out.edges.push(PlacedEdge {
            id: inst.edge.clone(),
            source: ctx.placements[inst.source].id.clone(),
            dest: ctx.placements[inst.dest].id.clone(),
            points: points,
            label: label,
            reversed: path.reversed,
        });
    }
    return out;
}

pub(crate) fn layout_container(ctx: &mut Ctx, container: Option<usize>, exits: Vec<ExitInfo>) -> ContainerResult {
    let members: Vec<usize> =
        (0 .. ctx.placements.len()).filter(|i| ctx.placements[*i].parent == container).collect();
    let member_index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, m)| (*m, i)).collect();

    struct Internal {
        dest: usize,
        inst: usize,
        source: usize,
    }

    let mut internal: Vec<Internal> = vec![];
    let mut self_loops: Vec<(usize, usize)> = vec![];
    let mut pass_through: Vec<(usize, usize)> = vec![];
    for (i, inst) in ctx.instances.iter().enumerate() {
        let s = ctx.ancestor_in(inst.source, container);
        let d = ctx.ancestor_in(inst.dest, container);
        match (s, d) {
            (Some(s), Some(d)) if s != d => {
                internal.push(Internal {
                    inst: i,
                    source: s,
                    dest: d,
                });
            },
            (Some(s), Some(d)) => {
                if inst.source == s && inst.dest == d {
                    self_loops.push((i, s));
                } else {
                    pass_through.push((i, s));
                }
            },
            _ => { },
        }
    }
    let adjacency: Vec<(usize, usize)> =
        internal.iter().map(|e| (member_index[&e.source], member_index[&e.dest])).collect();
    let prev_order: Vec<f64> =
        members.iter().map(|m| ctx.prev_x.get(&ctx.placements[*m].id).cloned().unwrap_or(f64::MAX)).collect();
    let ranking = rank::rank(members.len(), &adjacency, &prev_order);
    let mut child_exits: HashMap<usize, Vec<ExitInfo>> = HashMap::new();
    for e in &internal {
        let rs = ranking.rank[member_index[&e.source]];
        let rd = ranking.rank[member_index[&e.dest]];
        let (source_side, dest_side) = if rs < rd {
            (Side::After, Side::Before)
        } else {
            (Side::Before, Side::After)
        };
        let inst = &ctx.instances[e.inst];
        if ctx.is_container(e.source) {
            if let Some(inner) = ctx.ancestor_in(inst.source, Some(e.source)) {
                child_exits.entry(e.source).or_default().push(ExitInfo {
                    inst: e.inst,
                    member: inner,
                    side: source_side,
                    outgoing: true,
                    to_self: false,
                });
            }
        }
        if ctx.is_container(e.dest) {
            if let Some(inner) = ctx.ancestor_in(inst.dest, Some(e.dest)) {
                child_exits.entry(e.dest).or_default().push(ExitInfo {
                    inst: e.inst,
                    member: inner,
                    side: dest_side,
                    outgoing: false,
                    to_self: false,
                });
            }
        }
    }
    for x in &exits {
        if ctx.is_container(x.member) {
            let inst = &ctx.instances[x.inst];
            let endpoint = if x.outgoing {
                inst.source
            } else {
                inst.dest
            };
            if let Some(inner) = ctx.ancestor_in(endpoint, Some(x.member)) {
                child_exits.entry(x.member).or_default().push(ExitInfo {
                    inst: x.inst,
                    member: inner,
                    side: x.side,
                    outgoing: x.outgoing,
                    to_self: false,
                });
            }
        }
    }
    for (i, m) in &pass_through {
        let inst = &ctx.instances[*i];
        if inst.source == *m {
            if let Some(inner) = ctx.ancestor_in(inst.dest, Some(*m)) {
                child_exits.entry(*m).or_default().push(ExitInfo {
                    inst: *i,
                    member: inner,
                    side: ctx.config.flow.self_exit_side(),
                    outgoing: false,
                    to_self: true,
                });
            }
        } else if inst.dest == *m {
            if let Some(inner) = ctx.ancestor_in(inst.source, Some(*m)) {
                child_exits.entry(*m).or_default().push(ExitInfo {
                    inst: *i,
                    member: inner,
                    side: ctx.config.flow.self_exit_side(),
                    outgoing: true,
                    to_self: true,
                });
            }
        }
    }
    for m in &members {
        if ctx.is_container(*m) {
            let ex = child_exits.remove(m).unwrap_or_default();
            let res = layout_container(ctx, Some(*m), ex);
            ctx.results.insert(Some(*m), res);
        }
    }
    let ranking = (|| {
        let mut ranking = ranking;
        let Some(max_width) = ctx.config.max_rank_width else {
            return ranking;
        };
        let gap = ctx.config.node_gap;
        for (island_i, island_members) in ranking.islands.iter().enumerate() {
            let n_ranks = ranking.n_ranks[island_i];
            let mut by_rank: Vec<Vec<usize>> = vec![
                vec![];
                n_ranks
            ];
            for m in island_members {
                by_rank[ranking.rank[*m]].push(*m);
            }
            let mut new_rank = 0;
            for rank_members in &mut by_rank {
                rank_members.sort_by(|a, b| {
                    let key = |m: &usize| {
                        let id = &ctx.placements[members[*m]].id;
                        (
                            ctx.prev_y.get(id).cloned().unwrap_or(f64::MAX),
                            ctx.prev_x.get(id).cloned().unwrap_or(f64::MAX),
                        )
                    };
                    let (ya, xa) = key(a);
                    let (yb, xb) = key(b);
                    ya.partial_cmp(&yb).unwrap().then(xa.partial_cmp(&xb).unwrap()).then(a.cmp(b))
                });
                let mut width = 0.;
                let mut first = true;
                for m in rank_members.iter() {
                    let pl = members[*m];
                    let w =
                        ctx
                            .results
                            .get(&Some(pl))
                            .map(|r| r.size.width)
                            .unwrap_or(ctx.placements[pl].size.width);
                    let next = if first {
                        w
                    } else {
                        width + gap + w
                    };
                    if !first && next > max_width {
                        new_rank += 1;
                        width = w;
                    } else {
                        width = next;
                    }
                    first = false;
                    ranking.rank[*m] = new_rank;
                }
                new_rank += 1;
            }
            ranking.n_ranks[island_i] = new_rank;
        }
        return ranking;
    })();
    let internal_edges: Vec<(usize, usize, usize)> =
        internal.iter().map(|e| (e.inst, e.source, e.dest)).collect();
    let members: &[usize] = &members;
    let ranking: &rank::Ranking = &ranking;
    let internal: &[(usize, usize, usize)] = &internal_edges;
    let exits: &[ExitInfo] = &exits;
    let self_loops: &[(usize, usize)] = &self_loops;
    let config = ctx.config;
    let member_index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, m)| (*m, i)).collect();
    let mut port_counts: HashMap<(usize, Side), usize> = HashMap::new();
    for (_, s, d) in internal {
        let rs = ranking.rank[member_index[s]];
        let rd = ranking.rank[member_index[d]];
        let (ss, ds) = if rs < rd {
            (Side::After, Side::Before)
        } else {
            (Side::Before, Side::After)
        };
        *port_counts.entry((*s, ss)).or_default() += 1;
        *port_counts.entry((*d, ds)).or_default() += 1;
    }
    for x in exits {
        *port_counts.entry((x.member, x.side)).or_default() += 1;
    }
    for (_, m) in self_loops {
        *port_counts.entry((*m, Side::Before)).or_default() += 1;
        *port_counts.entry((*m, Side::After)).or_default() += 1;
    }
    let mut sizes: HashMap<usize, NodeSize> = HashMap::new();
    let mut insets: Vec<(usize, f64)> = vec![];
    for m in members {
        let mut size = match ctx.results.get(&Some(*m)) {
            Some(res) => res.size,
            None => ctx.placements[*m].size,
        };
        let ports =
            port_counts
                .get(&(*m, Side::Before))
                .cloned()
                .unwrap_or(0)
                .max(port_counts.get(&(*m, Side::After)).cloned().unwrap_or(0));
        if ports > 0 {
            let min_w = 2. * config.port_margin + (ports as f64 - 1.) * config.port_gap;
            if min_w > size.width {
                if ctx.results.contains_key(&Some(*m)) {
                    insets.push((*m, (min_w - size.width) / 2.));
                }
                size.width = min_w;
            }
        }
        sizes.insert(*m, size);
    }
    for (m, inset) in insets {
        let res = ctx.results.get_mut(&Some(m)).unwrap();
        res.inset = inset;
        res.size.width = sizes[&m].width;
    }
    let mut islands: Vec<position::IslandLayout> = vec![];
    for (island_i, island_members) in ranking.islands.iter().enumerate() {
        let n_ranks = ranking.n_ranks[island_i];
        let mut l = order::Layered::default();
        l.ranks = vec![
            vec![];
            n_ranks + 2
        ];
        let mut lnode_of: HashMap<usize, usize> = HashMap::new();
        for m in island_members {
            let m = members[*m];
            let size = sizes[&m];
            let i = l.add_node(order::LNode {
                kind: order::LKind::Real(m),
                rank: ranking.rank[member_index[&m]] + 1,
                w: size.width,
                h: size.height,
                x: 0.,
                prev_x: ctx.prev_x.get(&ctx.placements[m].id).cloned(),
                up: vec![],
                down: vec![],
            });
            lnode_of.insert(m, i);
        }
        let mut chains: Vec<position::Chain> = vec![];
        for (inst, s, d) in internal {
            let (Some(ls), Some(ld)) = (lnode_of.get(s), lnode_of.get(d)) else {
                continue;
            };
            let weight = ctx.instances[*inst].weight;
            let source_upper = l.nodes[*ls].rank < l.nodes[*ld].rank;
            let (upper, lower) = if source_upper {
                (*ls, *ld)
            } else {
                (*ld, *ls)
            };
            let edges = position::add_chain(&mut l, *inst, upper, lower, weight);
            chains.push(position::Chain {
                inst: *inst,
                edges: edges,
                kind: position::ChainKind::Internal { source_upper: source_upper },
            });
        }
        for x in exits {
            let Some(lm) = lnode_of.get(&x.member) else {
                continue;
            };
            let weight = ctx.instances[x.inst].weight;
            let exit_rank = match x.side {
                Side::Before => 0,
                Side::After => n_ranks + 1,
            };
            let exit = l.add_node(order::LNode {
                kind: order::LKind::Exit {
                    inst: x.inst,
                    side: x.side,
                    to_self: x.to_self,
                },
                rank: exit_rank,
                w: 0.,
                h: 0.,
                x: 0.,
                prev_x: None,
                up: vec![],
                down: vec![],
            });
            let (upper, lower) = match x.side {
                Side::Before => (exit, *lm),
                Side::After => (*lm, exit),
            };
            let edges = position::add_chain(&mut l, x.inst, upper, lower, weight);
            chains.push(position::Chain {
                inst: x.inst,
                edges: edges,
                kind: position::ChainKind::Exit {
                    side: x.side,
                    outgoing: x.outgoing,
                    to_self: x.to_self,
                },
            });
        }
        order::reduce_crossings(&mut l, config);
        {
            for r in 0 .. l.ranks.len() {
                let mut x = 0.;
                for (i, n) in l.ranks[r].clone().iter().enumerate() {
                    if i > 0 {
                        let prev = l.ranks[r][i - 1];
                        x += position::separation(&l.nodes[prev], &l.nodes[*n], config);
                    }
                    l.nodes[*n].x = x;
                }
            }
            let widths: Vec<f64> =
                (0 .. l.ranks.len()).map(|r| l.ranks[r].last().map(|n| l.nodes[*n].x).unwrap_or(0.)).collect();
            let max_w = widths.iter().cloned().fold(0., f64::max);
            for r in 0 .. l.ranks.len() {
                let shift = (max_w - widths[r]) / 2.;
                for n in &l.ranks[r] {
                    l.nodes[*n].x += shift;
                }
            }
            let n_ranks = l.ranks.len();
            for _ in 0 .. config.position_iters {
                for r in 1 .. n_ranks {
                    position::relax_rank(&mut l, r, true, false, config);
                }
                for r in (0 .. n_ranks.saturating_sub(1)).rev() {
                    position::relax_rank(&mut l, r, false, true, config);
                }
            }
            for r in 0 .. n_ranks {
                position::relax_rank(&mut l, r, true, true, config);
            }
        }
        {
            for ni in 0 .. l.nodes.len() {
                let node = l.nodes[ni].clone();
                match node.kind {
                    order::LKind::Real(m) => {
                        let fixed = ctx.results.get(&Some(m)).map(|r| (r.exit_ports.clone(), r.inset));
                        for (down, edges) in [(true, node.down.clone()), (false, node.up.clone())] {
                            if edges.is_empty() {
                                continue;
                            }
                            let left = node.x - node.w / 2.;
                            if let Some((fixed, inset)) = &fixed {
                                for e in &edges {
                                    let inst = l.edges[*e].inst;
                                    let x = fixed.get(&inst).map(|(_, x)| left + inset + x).unwrap_or(node.x);
                                    if down {
                                        l.edges[*e].x_upper = x;
                                    } else {
                                        l.edges[*e].x_lower = x;
                                    }
                                }
                                continue;
                            }
                            let mut keyed: Vec<(f64, usize)> = edges.iter().map(|e| {
                                let other = if down {
                                    l.edges[*e].lower
                                } else {
                                    l.edges[*e].upper
                                };
                                (l.nodes[other].x, *e)
                            }).collect();
                            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
                            let n = keyed.len() as f64;
                            let spacing = if keyed.len() > 1 {
                                config.port_gap.min((node.w - 2. * config.port_margin) / (n - 1.))
                            } else {
                                0.
                            };
                            for (i, (_, e)) in keyed.iter().enumerate() {
                                let x = node.x + (i as f64 - (n - 1.) / 2.) * spacing;
                                if down {
                                    l.edges[*e].x_upper = x;
                                } else {
                                    l.edges[*e].x_lower = x;
                                }
                            }
                        }
                    },
                    _ => {
                        for e in &node.down {
                            l.edges[*e].x_upper = node.x;
                        }
                        for e in &node.up {
                            l.edges[*e].x_lower = node.x;
                        }
                    },
                }
            }
        }
        let (rank_top, rank_height, track_y) = {
            let n_ranks = l.ranks.len();
            let mut rank_height = vec![
                0.;
                n_ranks
            ];
            for (r, rank) in l.ranks.iter().enumerate() {
                rank_height[r] = rank.iter().map(|n| l.nodes[*n].h).fold(0., f64::max);
            }
            let mut n_tracks = vec![
                0usize;
                n_ranks
            ];
            for r in 0 .. n_ranks {
                struct Seg {
                    edge: usize,
                    key: f64,
                    left: f64,
                    right: f64,
                }

                let mut segs: Vec<Seg> = vec![];
                for n in &l.ranks[r] {
                    for e in &l.nodes[*n].down {
                        let edge = &l.edges[*e];
                        if (edge.x_upper - edge.x_lower).abs() < 0.01 {
                            continue;
                        }
                        let rightward = edge.x_lower > edge.x_upper;
                        segs.push(Seg {
                            edge: *e,
                            left: edge.x_upper.min(edge.x_lower),
                            right: edge.x_upper.max(edge.x_lower),
                            key: if rightward {
                                -edge.x_upper
                            } else {
                                1e9 + edge.x_upper
                            },
                        });
                    }
                }
                segs.sort_by(
                    |a, b| a.key.partial_cmp(&b.key).unwrap().then(a.left.partial_cmp(&b.left).unwrap()),
                );
                let mut tracks: Vec<Vec<(f64, f64)>> = vec![];
                for s in &segs {
                    let mut assigned = None;
                    for (ti, t) in tracks.iter_mut().enumerate() {
                        let free =
                            t
                                .iter()
                                .all(
                                    |(tl, tr)| s.right + config.port_gap / 2. < *tl ||
                                        s.left - config.port_gap / 2. > *tr,
                                );
                        if free {
                            t.push((s.left, s.right));
                            assigned = Some(ti);
                            break;
                        }
                    }
                    let ti = match assigned {
                        Some(ti) => ti,
                        None => {
                            tracks.push(vec![(s.left, s.right)]);
                            tracks.len() - 1
                        },
                    };
                    l.edges[s.edge].track = Some(ti);
                }
                n_tracks[r] = tracks.len();
            }
            let mut rank_top = vec![
                0.;
                n_ranks
            ];
            let mut track_y = vec![
                vec![];
                n_ranks
            ];
            let mut y = 0.;
            for r in 0 .. n_ranks {
                rank_top[r] = y;
                y += rank_height[r];
                if r + 1 < n_ranks {
                    let has_edges = l.ranks[r].iter().any(|n| !l.nodes[*n].down.is_empty());
                    let gap = if !has_edges {
                        if l.ranks[r].is_empty() || l.ranks[r + 1].is_empty() {
                            0.
                        } else {
                            config.rank_gap
                        }
                    } else {
                        config.rank_gap + (n_tracks[r].max(1) - 1) as f64 * config.track_gap
                    };
                    for t in 0 .. n_tracks[r] {
                        track_y[r].push(y + config.rank_gap / 2. + t as f64 * config.track_gap);
                    }
                    y += gap;
                }
            }
            (rank_top, rank_height, track_y)
        };
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        for n in &l.nodes {
            min_x = min_x.min(n.x - n.w / 2.);
            max_x = max_x.max(n.x + n.w / 2.);
        }
        for e in &l.edges {
            min_x = min_x.min(e.x_upper.min(e.x_lower));
            max_x = max_x.max(e.x_upper.max(e.x_lower));
        }
        if min_x == f64::MAX {
            min_x = 0.;
            max_x = 0.;
        }
        for n in &mut l.nodes {
            n.x -= min_x;
        }
        for e in &mut l.edges {
            e.x_upper -= min_x;
            e.x_lower -= min_x;
        }
        let height = rank_top.last().cloned().unwrap_or(0.) + rank_height.last().cloned().unwrap_or(0.);
        let mut real_ranks = vec![];
        for r in 1 .. n_ranks + 1 {
            real_ranks.push(l.ranks[r].iter().filter_map(|n| match l.nodes[*n].kind {
                order::LKind::Real(m) => Some(m),
                _ => None,
            }).collect());
        }
        islands.push(position::IslandLayout {
            layered: l,
            chains: chains,
            rank_top: rank_top,
            rank_height: rank_height,
            track_y: track_y,
            width: max_x - min_x,
            height: height,
            real_ranks: real_ranks,
        });
    }
    let mut content_w = 0.;
    let mut content_h: f64 = 0.;
    let mut island_x = vec![];
    for (i, island) in islands.iter().enumerate() {
        if i > 0 {
            content_w += config.island_gap;
        }
        island_x.push(content_w);
        content_w += island.width;
        content_h = content_h.max(island.height);
    }
    let (origin, size, title_height) = match container {
        None => (pt(0., 0.), NodeSize {
            width: content_w,
            height: content_h,
        }, 0.),
        Some(c) => {
            let text = config.flow.canonical_size(ctx.placements[c].size);
            let pad = config.container_pad;
            match config.flow.title_at() {
                TitleAt::RankStart => {
                    let inner_w = content_w.max(text.width);
                    (pt(pad + (inner_w - content_w) / 2., text.height + config.title_gap), NodeSize {
                        width: inner_w + 2. * pad,
                        height: text.height + config.title_gap + content_h + pad,
                    }, text.height)
                },
                TitleAt::RankEnd => {
                    let inner_w = content_w.max(text.width);
                    (pt(pad + (inner_w - content_w) / 2., pad), NodeSize {
                        width: inner_w + 2. * pad,
                        height: pad + content_h + config.title_gap + text.height,
                    }, text.height)
                },
                TitleAt::SideStart => {
                    let inner_h = content_h.max(text.width);
                    (pt(text.height + config.title_gap, pad + (inner_h - content_h) / 2.), NodeSize {
                        width: text.height + config.title_gap + content_w + pad,
                        height: inner_h + 2. * pad,
                    }, text.height)
                },
            }
        },
    };
    let title_end = |last: Pt| -> Pt {
        match config.flow.title_at() {
            TitleAt::RankStart => pt(last.x, title_height),
            TitleAt::RankEnd => pt(last.x, size.height - title_height),
            TitleAt::SideStart => pt(title_height, last.y),
        }
    };
    let mut result = ContainerResult {
        size: size,
        title_height: title_height,
        inset: 0.,
        members: vec![],
        exit_ports: HashMap::new(),
        islands: vec![],
    };
    for (island_i, island) in islands.iter().enumerate() {
        let off = pt(origin.x + island_x[island_i], origin.y);
        let l = &island.layered;
        let mut real_index: HashMap<usize, usize> = HashMap::new();
        for (r, rank) in island.real_ranks.iter().enumerate() {
            for (i, m) in rank.iter().enumerate() {
                real_index.insert(*m, i);
                let _ = r;
            }
        }
        for (ni, n) in l.nodes.iter().enumerate() {
            match n.kind {
                order::LKind::Real(m) => {
                    let top = island.rank_top[n.rank] + (island.rank_height[n.rank] - n.h) / 2.;
                    result.members.push((m, Rect {
                        x: off.x + n.x - n.w / 2.,
                        y: off.y + top,
                        w: n.w,
                        h: n.h,
                    }, NavInfo {
                        island: island_i,
                        rank: n.rank - 1,
                        index: real_index[&m],
                    }));
                    let _ = ni;
                },
                order::LKind::Exit { inst, side, .. } => {
                    result.exit_ports.insert(inst, (side, off.x + n.x));
                },
                order::LKind::Dummy => { },
            }
        }
        result.islands.push(island.real_ranks.clone());
        for chain in &island.chains {
            let mut points: Vec<Pt> = vec![];
            for (i, ei) in chain.edges.iter().enumerate() {
                let e = &l.edges[*ei];
                let upper = &l.nodes[e.upper];
                let lower = &l.nodes[e.lower];
                let y_upper = position::node_top(island, upper) + upper.h;
                let y_lower = position::node_top(island, lower);
                if i == 0 {
                    points.push(pt(e.x_upper, y_upper));
                }
                if (e.x_upper - e.x_lower).abs() > 0.01 {
                    let ty = island.track_y[upper.rank][e.track.unwrap_or(0)];
                    points.push(pt(e.x_upper, ty));
                    points.push(pt(e.x_lower, ty));
                }
                points.push(pt(e.x_lower, y_lower));
            }
            let mut points: Vec<Pt> = points.into_iter().map(|p| pt(off.x + p.x, off.y + p.y)).collect();
            match chain.kind {
                position::ChainKind::Internal { source_upper } => {
                    if !source_upper {
                        points.reverse();
                    }
                    ctx.paths[chain.inst].main = Some(PathPiece {
                        container: container,
                        points: points,
                    });
                    ctx.paths[chain.inst].reversed = !source_upper;
                },
                position::ChainKind::Exit { side, outgoing, to_self } => {
                    if side == Side::Before {
                        points.reverse();
                    }
                    let last = points.last().cloned().unwrap();
                    if to_self {
                        points.push(title_end(last));
                    } else if side == Side::Before {
                        points.push(pt(last.x, 0.));
                    } else {
                        points.push(pt(last.x, size.height));
                    }
                    let piece = PathPiece {
                        container: container,
                        points: points,
                    };
                    if outgoing {
                        ctx.paths[chain.inst].source_stubs.push(piece);
                    } else {
                        ctx.paths[chain.inst].dest_stubs.push(piece);
                    }
                    if to_self {
                        ctx.paths[chain.inst].main = Some(PathPiece {
                            container: container,
                            points: vec![],
                        });
                    }
                },
            }
        }
    }
    for (inst, m) in self_loops {
        let Some((_, rect, _)) = result.members.iter().find(|(mm, _, _)| mm == m) else {
            continue;
        };
        let d = config.port_gap;
        let x = rect.right() - config.port_margin;
        let points =
            vec![
                pt(x, rect.bottom()),
                pt(x, rect.bottom() + d),
                pt(rect.right() + d, rect.bottom() + d),
                pt(rect.right() + d, rect.y - d),
                pt(x, rect.y - d),
                pt(x, rect.y),
            ];
        ctx.paths[*inst].main = Some(PathPiece {
            container: container,
            points: points,
        });
    }
    return result;
}

#[derive(Clone, Debug)]
pub struct LayoutConfig {
    pub container_pad: f64,
    pub flow: Flow,
    pub island_gap: f64,
    pub max_rank_width: Option<f64>,
    pub node_gap: f64,
    pub order_sweeps: usize,
    pub port_gap: f64,
    pub port_margin: f64,
    pub position_iters: usize,
    pub rank_gap: f64,
    pub selected_weight: f64,
    pub title_gap: f64,
    pub track_gap: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        return LayoutConfig {
            node_gap: 28.,
            rank_gap: 36.,
            track_gap: 12.,
            island_gap: 56.,
            container_pad: 14.,
            title_gap: 6.,
            port_gap: 14.,
            port_margin: 10.,
            order_sweeps: 6,
            position_iters: 6,
            selected_weight: 4.,
            max_rank_width: None,
            flow: Flow::Down,
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    Backward,
    Forward,
    SideNext,
    SidePrev,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavInfo {
    pub index: usize,
    pub island: usize,
    pub rank: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NodeSize {
    pub height: f64,
    pub width: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct PathPiece {
    pub container: Option<usize>,
    pub points: Vec<Pt>,
}

#[derive(Clone, Debug)]
pub struct PlacedEdge {
    pub dest: PlacementId,
    pub id: EdgeId,
    pub label: Pt,
    pub points: Vec<Pt>,
    pub reversed: bool,
    pub source: PlacementId,
}

#[derive(Clone, Debug)]
pub struct PlacedNode {
    pub container: bool,
    pub depth: usize,
    pub ghost: bool,
    pub id: PlacementId,
    pub nav: NavInfo,
    pub rect: Rect,
    pub title_height: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct Placement {
    pub ghost: bool,
    pub id: PlacementId,
    pub parent: Option<usize>,
    pub size: NodeSize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlacementId {
    pub container: Option<NodeId>,
    pub node: NodeId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Port {
    pub along: f64,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

pub fn pt(x: f64, y: f64) -> Pt {
    return Pt {
        x: x,
        y: y,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub h: f64,
    pub w: f64,
    pub x: f64,
    pub y: f64,
}

impl Rect {
    pub fn bottom(&self) -> f64 {
        return self.y + self.h;
    }

    pub fn cx(&self) -> f64 {
        return self.x + self.w / 2.;
    }

    pub fn cy(&self) -> f64 {
        return self.y + self.h / 2.;
    }

    pub fn right(&self) -> f64 {
        return self.x + self.w;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenDir {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    After,
    Before,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitleAt {
    RankEnd,
    RankStart,
    SideStart,
}
