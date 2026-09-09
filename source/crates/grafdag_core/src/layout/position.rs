//! Horizontal/vertical coordinate assignment, ports, tracks, routing and
//! island packing for one container.
use {
    super::{
        order::{
            reduce_crossings,
            LKind,
            LNode,
            Layered,
        },
        pt,
        rank::Ranking,
        ContainerResult,
        Ctx,
        ExitInfo,
        NavInfo,
        NodeSize,
        PathPiece,
        Pt,
        Rect,
        Side,
        TitleAt,
    },
    std::collections::HashMap,
};

struct Chain {
    inst: usize,
    /// Layered edge indices from the upper end to the lower end.
    edges: Vec<usize>,
    kind: ChainKind,
}

enum ChainKind {
    /// Source is the upper end (false: source is the lower end).
    Internal {
        source_upper: bool,
    },
    Exit {
        side: Side,
        outgoing: bool,
        to_self: bool,
    },
}

struct IslandLayout {
    layered: Layered,
    chains: Vec<Chain>,
    /// Top y per rank.
    rank_top: Vec<f64>,
    rank_height: Vec<f64>,
    /// Y per track per gap (gap r is between rank r and r + 1).
    track_y: Vec<Vec<f64>>,
    width: f64,
    height: f64,
    /// Real ranks (members only), for navigation.
    real_ranks: Vec<Vec<usize>>,
}

pub(crate) fn position_container(
    ctx: &mut Ctx,
    container: Option<usize>,
    members: &[usize],
    ranking: &Ranking,
    internal: &[(usize, usize, usize)],
    exits: &[ExitInfo],
    self_loops: &[(usize, usize)],
) -> ContainerResult {
    let config = ctx.config;
    let member_index: HashMap<usize, usize> = members.iter().enumerate().map(|(i, m)| (*m, i)).collect();

    // Port counts per member side, for minimum widths
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
    // Node sizes: the text box (or, for a container, the box its own layout
    // came to) widened to fit the edge ports on its busiest side. A widened
    // container centers its contents in the box (`inset`).
    let mut sizes: HashMap<usize, NodeSize> = HashMap::new();
    let mut insets: Vec<(usize, f64)> = vec![];
    for m in members {
        let mut size = match ctx.results.get(&Some(*m)) {
            Some(res) => res.size,
            None => ctx.placements[*m].size,
        };
        let ports =
            port_counts.get(&(*m, Side::Before)).cloned().unwrap_or(0).max(port_counts.get(&(*m, Side::After)).cloned().unwrap_or(0));
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

    // Lay out each island
    let mut islands: Vec<IslandLayout> = vec![];
    for (island_i, island_members) in ranking.islands.iter().enumerate() {
        let n_ranks = ranking.n_ranks[island_i];
        let mut l = Layered::default();
        // Ensure all ranks exist, including exit ranks
        l.ranks = vec![vec![]; n_ranks + 2];
        let mut lnode_of: HashMap<usize, usize> = HashMap::new();
        for m in island_members {
            let m = members[*m];
            let size = sizes[&m];
            let i = l.add_node(LNode {
                kind: LKind::Real(m),
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
        let mut chains: Vec<Chain> = vec![];
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
            let edges = add_chain(&mut l, *inst, upper, lower, weight);
            chains.push(Chain {
                inst: *inst,
                edges: edges,
                kind: ChainKind::Internal { source_upper: source_upper },
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
            let exit = l.add_node(LNode {
                kind: LKind::Exit {
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
            let edges = add_chain(&mut l, x.inst, upper, lower, weight);
            chains.push(Chain {
                inst: x.inst,
                edges: edges,
                kind: ChainKind::Exit {
                    side: x.side,
                    outgoing: x.outgoing,
                    to_self: x.to_self,
                },
            });
        }

        reduce_crossings(&mut l, config);
        assign_x(&mut l, config);
        assign_ports(ctx, &mut l);
        let (rank_top, rank_height, track_y) = assign_tracks_and_y(&mut l, config);

        // Normalize x so the island starts at 0
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
                LKind::Real(m) => Some(m),
                _ => None,
            }).collect());
        }
        islands.push(IslandLayout {
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

    // Pack islands horizontally
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

    // Container box. The title strip is at the top of the box on screen; where
    // that is in the canonical frame depends on the flow.
    let (origin, size, title_height) = match container {
        None => (pt(0., 0.), NodeSize {
            width: content_w,
            height: content_h,
        }, 0.),
        Some(c) => {
            // Title text box, back in screen terms (placements hold canonical
            // sizes): the title is always drawn horizontally across the top of
            // the box, and the strip is as thick as the text is tall.
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
                    // The strip is a band at the start of the side axis (the
                    // top of the box on screen): as thick as the text's screen
                    // height, and long enough for its screen width.
                    let inner_h = content_h.max(text.width);
                    (pt(text.height + config.title_gap, pad + (inner_h - content_h) / 2.), NodeSize {
                        width: text.height + config.title_gap + content_w + pad,
                        height: inner_h + 2. * pad,
                    }, text.height)
                },
            }
        },
    };
    // Where an edge to the container itself ends (the title strip's inner
    // edge), given the last point of its stub
    let title_end = |last: Pt| -> Pt {
        match config.flow.title_at() {
            TitleAt::RankStart => pt(last.x, title_height),
            TitleAt::RankEnd => pt(last.x, size.height - title_height),
            TitleAt::SideStart => pt(title_height, last.y),
        }
    };

    // Emit results
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
                LKind::Real(m) => {
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
                LKind::Exit { inst, side, .. } => {
                    result.exit_ports.insert(inst, (side, off.x + n.x));
                },
                LKind::Dummy => { },
            }
        }
        result.islands.push(island.real_ranks.clone());

        // Edge paths
        for chain in &island.chains {
            let mut points: Vec<Pt> = vec![];
            for (i, ei) in chain.edges.iter().enumerate() {
                let e = &l.edges[*ei];
                let upper = &l.nodes[e.upper];
                let lower = &l.nodes[e.lower];
                let y_upper = node_bottom(island, upper);
                let y_lower = node_top(island, lower);
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
                ChainKind::Internal { source_upper } => {
                    if !source_upper {
                        points.reverse();
                    }
                    ctx.paths[chain.inst].main = Some(PathPiece {
                        container: container,
                        points: points,
                    });
                    ctx.paths[chain.inst].reversed = !source_upper;
                },
                ChainKind::Exit { side, outgoing, to_self } => {
                    // Orient from the member outwards
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

    // Self loops: around the right side of the node
    for (inst, m) in self_loops {
        let Some((_, rect, _)) = result.members.iter().find(|(mm, _, _)| mm == m) else {
            continue;
        };
        let d = config.port_gap;
        let x = rect.right() - config.port_margin;
        let points = vec![
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

fn node_top(island: &IslandLayout, n: &LNode) -> f64 {
    return island.rank_top[n.rank] + (island.rank_height[n.rank] - n.h) / 2.;
}

fn node_bottom(island: &IslandLayout, n: &LNode) -> f64 {
    return node_top(island, n) + n.h;
}

/// Add an edge chain between an upper and lower node, inserting dummies for
/// intermediate ranks. Returns the layered edges from top to bottom.
fn add_chain(l: &mut Layered, inst: usize, upper: usize, lower: usize, weight: f64) -> Vec<usize> {
    let mut edges = vec![];
    let mut prev = upper;
    let r_upper = l.nodes[upper].rank;
    let r_lower = l.nodes[lower].rank;
    for r in r_upper + 1 .. r_lower {
        let d = l.add_node(LNode {
            kind: LKind::Dummy,
            rank: r,
            w: 0.,
            h: 0.,
            x: 0.,
            prev_x: None,
            up: vec![],
            down: vec![],
        });
        edges.push(l.add_edge(inst, prev, d, weight));
        prev = d;
    }
    edges.push(l.add_edge(inst, prev, lower, weight));
    return edges;
}

/// Pool-adjacent-violators: weighted least squares non-decreasing fit.
fn pav(values: &[f64], weights: &[f64]) -> Vec<f64> {
    // (sum of w*v, sum of w, count)
    let mut blocks: Vec<(f64, f64, usize)> = vec![];
    for (v, w) in values.iter().zip(weights) {
        blocks.push((v * w, *w, 1));
        while blocks.len() >= 2 {
            let n = blocks.len();
            let (a, b) = (blocks[n - 2], blocks[n - 1]);
            if a.0 / a.1 > b.0 / b.1 {
                blocks.pop();
                blocks.pop();
                blocks.push((a.0 + b.0, a.1 + b.1, a.2 + b.2));
            } else {
                break;
            }
        }
    }
    let mut out = vec![];
    for b in blocks {
        for _ in 0 .. b.2 {
            out.push(b.0 / b.1);
        }
    }
    return out;
}

fn is_thin(n: &LNode) -> bool {
    return !matches!(n.kind, LKind::Real(_));
}

/// Assign horizontal centers: start packed, then repeatedly move nodes towards
/// the mean of their neighbours subject to ordering and spacing constraints.
fn assign_x(l: &mut Layered, config: &super::LayoutConfig) {
    for r in 0 .. l.ranks.len() {
        let mut x = 0.;
        for (i, n) in l.ranks[r].clone().iter().enumerate() {
            if i > 0 {
                let prev = l.ranks[r][i - 1];
                x += separation(&l.nodes[prev], &l.nodes[*n], config);
            }
            l.nodes[*n].x = x;
        }
    }
    // Center ranks initially
    let widths: Vec<f64> = (0 .. l.ranks.len()).map(|r| l.ranks[r].last().map(|n| l.nodes[*n].x).unwrap_or(0.)).collect();
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
            relax_rank(l, r, true, false, config);
        }
        for r in (0 .. n_ranks.saturating_sub(1)).rev() {
            relax_rank(l, r, false, true, config);
        }
    }
    for r in 0 .. n_ranks {
        relax_rank(l, r, true, true, config);
    }
}

fn separation(a: &LNode, b: &LNode, config: &super::LayoutConfig) -> f64 {
    let gap = if is_thin(a) && is_thin(b) {
        config.port_gap
    } else if is_thin(a) || is_thin(b) {
        config.node_gap / 2.
    } else {
        config.node_gap
    };
    return a.w / 2. + b.w / 2. + gap;
}

fn relax_rank(l: &mut Layered, r: usize, use_up: bool, use_down: bool, config: &super::LayoutConfig) {
    let rank = l.ranks[r].clone();
    if rank.is_empty() {
        return;
    }
    let mut desired = vec![];
    let mut weights = vec![];
    let mut offsets = vec![];
    let mut offset = 0.;
    for (i, n) in rank.iter().enumerate() {
        let node = &l.nodes[*n];
        if i > 0 {
            offset += separation(&l.nodes[rank[i - 1]], node, config);
        }
        offsets.push(offset);
        let mut sum = 0.;
        let mut wsum = 0.;
        if use_up {
            for e in &node.up {
                let e = &l.edges[*e];
                sum += l.nodes[e.upper].x * e.weight;
                wsum += e.weight;
            }
        }
        if use_down {
            for e in &node.down {
                let e = &l.edges[*e];
                sum += l.nodes[e.lower].x * e.weight;
                wsum += e.weight;
            }
        }
        if wsum > 0. {
            desired.push(sum / wsum - offset);
            let priority = if is_thin(node) {
                4.
            } else {
                wsum.max(1.)
            };
            weights.push(priority);
        } else {
            desired.push(node.x - offset);
            weights.push(0.05);
        }
    }
    let fitted = pav(&desired, &weights);
    for (i, n) in rank.iter().enumerate() {
        l.nodes[*n].x = fitted[i] + offsets[i];
    }
}

/// Assign port x positions on each node side.
fn assign_ports(ctx: &Ctx, l: &mut Layered) {
    let config = ctx.config;
    for ni in 0 .. l.nodes.len() {
        let node = l.nodes[ni].clone();
        match node.kind {
            LKind::Real(m) => {
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

/// Assign horizontal track slots in each rank gap and compute rank y
/// positions. Returns (rank_top, rank_height, track_y per gap).
fn assign_tracks_and_y(l: &mut Layered, config: &super::LayoutConfig) -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>) {
    let n_ranks = l.ranks.len();
    let mut rank_height = vec![0.; n_ranks];
    for (r, rank) in l.ranks.iter().enumerate() {
        rank_height[r] = rank.iter().map(|n| l.nodes[*n].h).fold(0., f64::max);
    }
    let mut n_tracks = vec![0usize; n_ranks];
    for r in 0 .. n_ranks {
        // Segments in the gap below rank r
        struct Seg {
            edge: usize,
            left: f64,
            right: f64,
            key: f64,
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
        segs.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap().then(a.left.partial_cmp(&b.left).unwrap()));
        let mut tracks: Vec<Vec<(f64, f64)>> = vec![];
        for s in &segs {
            let mut assigned = None;
            for (ti, t) in tracks.iter_mut().enumerate() {
                let free = t.iter().all(|(tl, tr)| s.right + config.port_gap / 2. < *tl || s.left - config.port_gap / 2. > *tr);
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
    let mut rank_top = vec![0.; n_ranks];
    let mut track_y = vec![vec![]; n_ranks];
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
    return (rank_top, rank_height, track_y);
}
