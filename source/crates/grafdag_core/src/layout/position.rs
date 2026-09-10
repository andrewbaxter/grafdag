use super::{
    order::{
        LKind,
        LNode,
        Layered,
    },
    Side,
};

pub(super) fn add_chain(l: &mut Layered, inst: usize, upper: usize, lower: usize, weight: f64) -> Vec<usize> {
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

pub(super) struct Chain {
    pub(super) edges: Vec<usize>,
    pub(super) inst: usize,
    pub(super) kind: ChainKind,
}

pub(super) enum ChainKind {
    Exit {
        side: Side,
        outgoing: bool,
        to_self: bool,
    },
    Internal {
        source_upper: bool,
    },
}

fn is_thin(n: &LNode) -> bool {
    return !matches!(n.kind, LKind::Real(_));
}

pub(crate) struct IslandLayout {
    pub(super) chains: Vec<Chain>,
    pub(super) height: f64,
    pub(super) layered: Layered,
    pub(super) rank_height: Vec<f64>,
    pub(super) rank_top: Vec<f64>,
    pub(super) real_ranks: Vec<Vec<usize>>,
    pub(super) track_y: Vec<Vec<f64>>,
    pub(super) width: f64,
}

pub(super) fn node_top(island: &IslandLayout, n: &LNode) -> f64 {
    return island.rank_top[n.rank] + (island.rank_height[n.rank] - n.h) / 2.;
}

pub(super) fn relax_rank(l: &mut Layered, r: usize, use_up: bool, use_down: bool, config: &super::LayoutConfig) {
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
    let fitted = {
        let (values, weights) = (&desired, &weights);
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
        out
    };
    for (i, n) in rank.iter().enumerate() {
        l.nodes[*n].x = fitted[i] + offsets[i];
    }
}

pub(super) fn separation(a: &LNode, b: &LNode, config: &super::LayoutConfig) -> f64 {
    let gap = if is_thin(a) && is_thin(b) {
        config.port_gap
    } else if is_thin(a) || is_thin(b) {
        config.node_gap / 2.
    } else {
        config.node_gap
    };
    return a.w / 2. + b.w / 2. + gap;
}
