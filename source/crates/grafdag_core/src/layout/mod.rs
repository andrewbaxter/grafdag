//! Layered ("Sugiyama" style) layout for compound DAGs.
//!
//! The layout is hierarchical: a container (the root, or a node with child
//! nodes) lays out its members with a layered layout, treating member
//! containers as single boxes whose sizes were computed by laying them out
//! first. Edges leaving a container are routed inside it to a virtual "exit"
//! node on the container border, whose position becomes a port for the
//! enclosing level.
//!
//! Layers are purely logical: they only affect layout by weighting crossings
//! of edges touching the selected layer more heavily.
//!
//! Steps per container:
//!
//! 1. Islands (connected components).
//! 2. Ranks (cycle breaking by edge reversal, longest path), midpoints for
//!    edges spanning ranks.
//! 3. Crossing reduction (barycenter sweeps + stable transposition).
//! 4. Horizontal positions (constrained least squares towards neighbours).
//! 5. Right-angle edge routing with per-gap tracks.
//! 6. Linear island packing.
pub mod rank;
pub mod order;
pub mod position;

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
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn cx(&self) -> f64 {
        return self.x + self.w / 2.;
    }

    pub fn cy(&self) -> f64 {
        return self.y + self.h / 2.;
    }

    pub fn right(&self) -> f64 {
        return self.x + self.w;
    }

    pub fn bottom(&self) -> f64 {
        return self.y + self.h;
    }
}

/// A side of a node along the rank axis: `Before` faces earlier ranks
/// (predecessors), `After` later ranks (successors). Which screen side that is
/// depends on the `Flow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Before,
    After,
}

/// Where an edge attaches to a node box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Port {
    pub side: Side,
    /// Position along the side, in the side axis direction (see `Flow`).
    pub along: f64,
}

/// The direction ranks flow on screen. The layout works in a canonical frame
/// with two perpendicular axes: the rank axis (successive ranks) and the side
/// axis (nodes within a rank, and islands). `Flow` maps that frame to the
/// screen: the rank axis points in the flow direction, and the side axis is
/// the perpendicular screen axis in its natural (rightward/downward) sense.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Flow {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

/// A direction on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenDir {
    Up,
    Down,
    Left,
    Right,
}

/// A movement in the canonical frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// Along the rank axis to later ranks.
    Forward,
    /// Along the rank axis to earlier ranks.
    Backward,
    /// Along the side axis in its positive sense.
    SideNext,
    SidePrev,
}

/// Where a container's title strip sits in the canonical frame (it's always
/// at the top of the box on screen, spanning its width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TitleAt {
    /// At the start of the rank axis.
    RankStart,
    /// At the end of the rank axis.
    RankEnd,
    /// At the start of the side axis.
    SideStart,
}

impl Flow {
    /// All flows, in counterclockwise order on screen.
    pub const ALL: [Flow; 4] = [Flow::Down, Flow::Right, Flow::Up, Flow::Left];

    /// The flow a quarter turn counterclockwise on screen.
    pub fn ccw(self) -> Flow {
        let i = Flow::ALL.iter().position(|f| *f == self).unwrap();
        return Flow::ALL[(i + 1) % Flow::ALL.len()];
    }

    /// The flow a quarter turn clockwise on screen.
    pub fn cw(self) -> Flow {
        let i = Flow::ALL.iter().position(|f| *f == self).unwrap();
        return Flow::ALL[(i + Flow::ALL.len() - 1) % Flow::ALL.len()];
    }

    /// The rank axis is the screen x axis.
    pub fn horizontal(self) -> bool {
        return matches!(self, Flow::Right | Flow::Left);
    }

    /// Convert a size between screen (width, height) and canonical (side
    /// extent, rank extent). The conversion is its own inverse.
    pub fn canonical_size(self, s: NodeSize) -> NodeSize {
        if self.horizontal() {
            return NodeSize {
                width: s.height,
                height: s.width,
            };
        }
        return s;
    }

    /// Canonical point (side, rank) to screen, given the canonical extents of
    /// the whole layout.
    pub fn to_screen(self, extent: NodeSize, p: Pt) -> Pt {
        return match self {
            Flow::Down => pt(p.x, p.y),
            Flow::Up => pt(p.x, extent.height - p.y),
            Flow::Right => pt(p.y, p.x),
            Flow::Left => pt(extent.height - p.y, p.x),
        };
    }

    /// Screen point to canonical (side, rank), given the canonical extents of
    /// the whole layout.
    pub fn to_canonical(self, extent: NodeSize, p: Pt) -> Pt {
        return match self {
            Flow::Down => pt(p.x, p.y),
            Flow::Up => pt(p.x, extent.height - p.y),
            Flow::Right => pt(p.y, p.x),
            Flow::Left => pt(p.y, extent.height - p.x),
        };
    }

    /// Canonical rect to screen.
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

    /// What a screen direction means in the canonical frame.
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

    /// The screen direction of a canonical movement.
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

    /// The screen side of a node an edge attaches to.
    pub fn screen_side(self, side: Side) -> ScreenDir {
        return self.screen_dir(match side {
            Side::Before => Motion::Backward,
            Side::After => Motion::Forward,
        });
    }

    pub(crate) fn title_at(self) -> TitleAt {
        return match self {
            Flow::Down => TitleAt::RankStart,
            Flow::Up => TitleAt::RankEnd,
            Flow::Right | Flow::Left => TitleAt::SideStart,
        };
    }

    /// The side of a container's members that edges to the container itself
    /// leave from (towards the title).
    pub(crate) fn self_exit_side(self) -> Side {
        return match self.title_at() {
            TitleAt::RankEnd => Side::After,
            _ => Side::Before,
        };
    }
}

#[derive(Clone, Debug)]
pub struct LayoutConfig {
    /// Horizontal gap between nodes in a rank.
    pub node_gap: f64,
    /// Minimum vertical gap between ranks.
    pub rank_gap: f64,
    /// Vertical spacing between horizontal edge tracks within a rank gap.
    pub track_gap: f64,
    /// Horizontal gap between islands.
    pub island_gap: f64,
    /// Padding inside a container around its children.
    pub container_pad: f64,
    /// Gap between a container's title text and its children.
    pub title_gap: f64,
    /// Minimum spacing between edge ports on one side of a node.
    pub port_gap: f64,
    /// Horizontal inset of the outermost ports from the node edge.
    pub port_margin: f64,
    /// Number of crossing-reduction sweeps.
    pub order_sweeps: usize,
    /// Number of horizontal position relaxation iterations.
    pub position_iters: usize,
    /// Crossing cost multiplier for edges touching the selected layer.
    pub selected_weight: f64,
    /// Ranks wider (along the side axis) than this are split into several
    /// ranks (e.g. the screen extent along that axis). None disables splitting.
    pub max_rank_width: Option<f64>,
    /// Screen direction of the rank axis.
    pub flow: Flow,
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

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NodeSize {
    pub width: f64,
    pub height: f64,
}

/// One drawn copy of a node. A node is laid out in its first visible parent
/// (its primary placement); it's drawn again ("split", ghost) in any other
/// visible parents.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlacementId {
    pub node: NodeId,
    pub container: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub struct PlacedNode {
    pub id: PlacementId,
    pub rect: Rect,
    /// Height of the text/title area at the top of the box.
    pub title_height: f64,
    pub ghost: bool,
    pub container: bool,
    /// Nesting depth (0 at the root).
    pub depth: usize,
    pub nav: NavInfo,
}

/// Position of a placement within the layered structure, for keyboard
/// navigation.
#[derive(Clone, Debug, PartialEq)]
pub struct NavInfo {
    /// Global island index (see `Layout::islands`).
    pub island: usize,
    /// Rank within the island (0 = first).
    pub rank: usize,
    /// Position within the rank, left to right.
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct PlacedEdge {
    pub id: EdgeId,
    pub source: PlacementId,
    pub dest: PlacementId,
    /// Right-angle polyline from the source port to the dest port.
    pub points: Vec<Pt>,
    /// Where to put the label.
    pub label: Pt,
    /// Whether the edge is drawn against the rank direction (dest above
    /// source).
    pub reversed: bool,
}

#[derive(Clone, Debug)]
pub struct Island {
    pub container: Option<NodeId>,
    /// Members by rank, left to right within each rank.
    pub ranks: Vec<Vec<PlacementId>>,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Screen size.
    pub width: f64,
    pub height: f64,
    pub flow: Flow,
    pub nodes: Vec<PlacedNode>,
    pub edges: Vec<PlacedEdge>,
    pub islands: Vec<Island>,
}

impl Layout {
    pub fn node(&self, id: &PlacementId) -> Option<&PlacedNode> {
        return self.nodes.iter().find(|n| &n.id == id);
    }

    /// The primary (non-ghost) placement of a node.
    pub fn primary(&self, id: &NodeId) -> Option<&PlacedNode> {
        return self.nodes.iter().find(|n| &n.id.node == id && !n.ghost);
    }

    /// Canonical extents (side, rank) of the whole layout.
    pub fn extent(&self) -> NodeSize {
        return self.flow.canonical_size(NodeSize {
            width: self.width,
            height: self.height,
        });
    }

    /// A screen point in the canonical frame: x is along the side axis, y
    /// along the rank axis.
    pub fn canonical(&self, p: Pt) -> Pt {
        return self.flow.to_canonical(self.extent(), p);
    }

    /// Where an edge attaches to a placement, as drawn. Edges on the same side
    /// of a node are ordered visually by `along`.
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
}

// Internal structures

#[derive(Clone, Debug)]
pub(crate) struct Placement {
    pub id: PlacementId,
    pub parent: Option<usize>,
    pub ghost: bool,
    pub size: NodeSize,
}

#[derive(Clone, Debug)]
pub(crate) struct EdgeInstance {
    pub edge: EdgeId,
    pub source: usize,
    pub dest: usize,
    pub weight: f64,
}

/// An edge crossing the border of a container, as seen from inside.
#[derive(Clone, Debug)]
pub(crate) struct ExitInfo {
    pub inst: usize,
    /// The member (of the container being laid out) the edge attaches to.
    pub member: usize,
    pub side: Side,
    /// The member is the source end of the edge.
    pub outgoing: bool,
    /// The other end is the container itself (drawn to the container title).
    pub to_self: bool,
}

/// A piece of an edge path in the local coordinates of a container.
#[derive(Clone, Debug)]
pub(crate) struct PathPiece {
    pub container: Option<usize>,
    pub points: Vec<Pt>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EdgePath {
    pub main: Option<PathPiece>,
    /// Stubs from the source end outwards, deepest first.
    pub source_stubs: Vec<PathPiece>,
    /// Stubs from the dest end outwards, deepest first.
    pub dest_stubs: Vec<PathPiece>,
    pub reversed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ContainerResult {
    /// Full box size (including title and padding for container nodes).
    pub size: NodeSize,
    pub title_height: f64,
    /// How far the contents (members, ports, paths) sit inside the box along
    /// the side axis. Non-zero when the enclosing layout widened the box to
    /// fit the node's edge ports, which centers the contents in it.
    pub inset: f64,
    /// Member rects in local coordinates (relative to the container box).
    pub members: Vec<(usize, Rect, NavInfo)>,
    /// Border ports for edges crossing this container, relative to the box.
    pub exit_ports: HashMap<usize, (Side, f64)>,
    pub islands: Vec<Vec<Vec<usize>>>,
}

pub(crate) struct Ctx<'a> {
    pub config: &'a LayoutConfig,
    pub placements: Vec<Placement>,
    pub instances: Vec<EdgeInstance>,
    pub prev_x: HashMap<PlacementId, f64>,
    pub prev_y: HashMap<PlacementId, f64>,
    pub paths: Vec<EdgePath>,
    pub results: HashMap<Option<usize>, ContainerResult>,
}

impl<'a> Ctx<'a> {
    /// The ancestor of `pl` (possibly itself) that is a direct member of
    /// `container`.
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

/// Lay out the visible part of the document. `sizes` gives the text box size
/// for every visible node (missing nodes get a default size). `titles`
/// overrides that for nodes drawn as containers, whose title is wrapped to the
/// container box rather than to its own aspect ratio; ghost copies of a
/// container (drawn as plain boxes) still use `sizes`. `previous` is used to
/// keep the ordering stable across edits.
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

    // Visible nodes
    let visible: Vec<&NodeId> = doc.nodes.iter().filter(|n| doc.node_visible(n)).map(|n| &n.id).collect();
    let visible_set: HashSet<&NodeId> = visible.iter().cloned().collect();

    // Primary parents, breaking containment cycles
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
                // Cycle: detach the node we started from
                primary_parent.insert((*id).clone(), None);
                break;
            }
            at = p;
        }
    }

    // Placements: primary first (so indices are stable), then ghosts
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

    // Layers per node
    let mut node_layers: HashMap<NodeId, HashSet<LayerId>> = HashMap::new();
    for n in &doc.nodes {
        node_layers.insert(n.id.clone(), n.layers.iter().cloned().collect());
    }
    let selected_layer = doc.selected_layer.clone().filter(|l| doc.layer_active(l));

    // Edge instances
    let mut instances = vec![];
    for e in &doc.edges {
        if !doc.edge_visible(e) {
            continue;
        }
        let s = primary_index[&e.source];
        let d = primary_index[&e.dest];
        let mut weight = 1.;
        if let Some(sel) = &selected_layer {
            let touches = e.layer.as_ref() == Some(sel) || node_layers[&e.source].contains(sel) || node_layers[&e.dest].contains(sel);
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
        // Previous positions in the canonical frame, so ordering survives a
        // change of flow
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
        paths: vec![EdgePath::default(); n_inst],
        results: HashMap::new(),
    };
    let root = layout_container(&mut ctx, None, vec![]);
    ctx.results.insert(None, root);
    return assemble(ctx);
}

/// Lay out one container. Ranks members, recurses into member containers, then
/// positions everything. Results are stored in `ctx.results`.
pub(crate) fn layout_container(ctx: &mut Ctx, container: Option<usize>, exits: Vec<ExitInfo>) -> ContainerResult {
    let members: Vec<usize> = (0 .. ctx.placements.len()).filter(|i| ctx.placements[*i].parent == container).collect();
    let member_index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, m)| (*m, i)).collect();

    // Classify edge instances
    struct Internal {
        inst: usize,
        source: usize,
        dest: usize,
    }

    let mut internal: Vec<Internal> = vec![];
    let mut self_loops: Vec<(usize, usize)> = vec![];
    // (inst, member the edge is inside of)
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

    // Islands and ranks
    let adjacency: Vec<(usize, usize)> = internal.iter().map(|e| (member_index[&e.source], member_index[&e.dest])).collect();
    let prev_order: Vec<f64> = members.iter().map(|m| ctx.prev_x.get(&ctx.placements[*m].id).cloned().unwrap_or(f64::MAX)).collect();
    let ranking = rank::rank(members.len(), &adjacency, &prev_order);

    // Exits for member containers
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

    // Recurse into member containers
    for m in &members {
        if ctx.is_container(*m) {
            let ex = child_exits.remove(m).unwrap_or_default();
            let res = layout_container(ctx, Some(*m), ex);
            ctx.results.insert(Some(*m), res);
        }
    }

    // Split ranks that are too wide, now that member sizes are known. Edges never
    // connect nodes in the same rank, so this keeps every edge's direction and the
    // exit sides computed above.
    let ranking = split_wide_ranks(ctx, &members, ranking);

    // Position
    let internal_edges: Vec<(usize, usize, usize)> = internal.iter().map(|e| (e.inst, e.source, e.dest)).collect();
    return position::position_container(ctx, container, &members, &ranking, &internal_edges, &exits, &self_loops);
}

fn member_width(ctx: &Ctx, pl: usize) -> f64 {
    return ctx.results.get(&Some(pl)).map(|r| r.size.width).unwrap_or(ctx.placements[pl].size.width);
}

/// Split ranks whose members don't fit in `max_rank_width` into consecutive
/// ranks, keeping the previous horizontal order where known.
fn split_wide_ranks(ctx: &Ctx, members: &[usize], mut ranking: rank::Ranking) -> rank::Ranking {
    let Some(max_width) = ctx.config.max_rank_width else {
        return ranking;
    };
    let gap = ctx.config.node_gap;
    for (island_i, island_members) in ranking.islands.iter().enumerate() {
        let n_ranks = ranking.n_ranks[island_i];
        let mut by_rank: Vec<Vec<usize>> = vec![vec![]; n_ranks];
        for m in island_members {
            by_rank[ranking.rank[*m]].push(*m);
        }
        let mut new_rank = 0;
        for rank_members in &mut by_rank {
            // Previous rows first, then previous x, so an existing split is kept
            rank_members.sort_by(|a, b| {
                let key = |m: &usize| {
                    let id = &ctx.placements[members[*m]].id;
                    (ctx.prev_y.get(id).cloned().unwrap_or(f64::MAX), ctx.prev_x.get(id).cloned().unwrap_or(f64::MAX))
                };
                let (ya, xa) = key(a);
                let (yb, xb) = key(b);
                ya.partial_cmp(&yb).unwrap().then(xa.partial_cmp(&xb).unwrap()).then(a.cmp(b))
            });
            let mut width = 0.;
            let mut first = true;
            for m in rank_members.iter() {
                let w = member_width(ctx, members[*m]);
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
}

/// Convert local container results into absolute coordinates.
fn assemble(ctx: Ctx) -> Layout {
    let mut out = Layout::default();
    let root = &ctx.results[&None];
    let flow = ctx.config.flow;
    let extent = root.size;
    let screen = flow.canonical_size(extent);
    out.width = screen.width;
    out.height = screen.height;
    out.flow = flow;

    // Islands: assign global indices in a deterministic order (root first)
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
                // The box may be wider than the contents (ports); they're
                // centered in it
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
                if points.last().map(|l: &Pt| (l.x - p.x).abs() < 0.01 && (l.y - p.y).abs() < 0.01).unwrap_or(false) {
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
        let points = simplify(points.into_iter().map(|p| flow.to_screen(extent, p)).collect());
        let label = label_position(&points);
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

/// Remove collinear intermediate points.
fn simplify(points: Vec<Pt>) -> Vec<Pt> {
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
    return out;
}

/// Middle of the longest segment, preferring horizontal segments.
fn label_position(points: &[Pt]) -> Pt {
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
    return best.unwrap_or(points.first().cloned().unwrap_or(pt(0., 0.)));
}
#[cfg(test)]
mod tests;
