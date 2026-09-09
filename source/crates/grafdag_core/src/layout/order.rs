use super::{
    LayoutConfig,
    Side,
};

fn barycenter_sort(l: &mut Layered, r: usize, use_upper: bool) {
    let pos = l.positions();
    let mut keyed: Vec<(f64, usize, usize)> = vec![];
    for (i, n) in l.ranks[r].iter().enumerate() {
        let node = &l.nodes[*n];
        let edges = if use_upper {
            &node.up
        } else {
            &node.down
        };
        let bary = if edges.is_empty() {
            i as f64
        } else {
            let mut sum = 0.;
            let mut wsum = 0.;
            for e in edges {
                let e = &l.edges[*e];
                let other = if use_upper {
                    e.upper
                } else {
                    e.lower
                };
                sum += pos[other] as f64 * e.weight;
                wsum += e.weight;
            }
            let other_len = if use_upper {
                l.ranks[r - 1].len()
            } else {
                l.ranks[r + 1].len()
            } as f64;
            (sum / wsum) * (l.ranks[r].len() as f64 / other_len.max(1.))
        };
        keyed.push((bary, i, *n));
    }
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    l.ranks[r] = keyed.into_iter().map(|k| k.2).collect();
}

#[derive(Clone, Debug, Default)]
pub struct Layered {
    pub edges: Vec<LEdge>,
    pub nodes: Vec<LNode>,
    pub ranks: Vec<Vec<usize>>,
}

impl Layered {
    pub fn add_edge(&mut self, inst: usize, upper: usize, lower: usize, weight: f64) -> usize {
        let i = self.edges.len();
        self.edges.push(LEdge {
            inst: inst,
            upper: upper,
            lower: lower,
            weight: weight,
            x_upper: 0.,
            x_lower: 0.,
            track: None,
        });
        self.nodes[upper].down.push(i);
        self.nodes[lower].up.push(i);
        return i;
    }

    pub fn add_node(&mut self, node: LNode) -> usize {
        let rank = node.rank;
        while self.ranks.len() <= rank {
            self.ranks.push(vec![]);
        }
        let i = self.nodes.len();
        self.nodes.push(node);
        self.ranks[rank].push(i);
        return i;
    }

    pub fn crossings_below(&self, r: usize, pos: &[usize]) -> f64 {
        if r + 1 >= self.ranks.len() {
            return 0.;
        }
        let mut segs: Vec<(usize, usize, f64)> = vec![];
        for n in &self.ranks[r] {
            for e in &self.nodes[*n].down {
                let e = &self.edges[*e];
                segs.push((pos[e.upper], pos[e.lower], e.weight));
            }
        }
        let mut total = 0.;
        for i in 0 .. segs.len() {
            for j in i + 1 .. segs.len() {
                let (a, b) = (&segs[i], &segs[j]);
                if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                    total += a.2 * b.2;
                }
            }
        }
        return total;
    }

    pub fn degree(&self, node: usize) -> usize {
        return self.nodes[node].up.len() + self.nodes[node].down.len();
    }

    fn positions(&self) -> Vec<usize> {
        let mut pos = vec![
            0;
            self.nodes.len()
        ];
        for r in &self.ranks {
            for (i, n) in r.iter().enumerate() {
                pos[*n] = i;
            }
        }
        return pos;
    }

    pub fn total_crossings(&self) -> f64 {
        let pos = self.positions();
        return (0 .. self.ranks.len()).map(|r| self.crossings_below(r, &pos)).sum();
    }
}

#[derive(Clone, Debug)]
pub struct LEdge {
    pub inst: usize,
    pub lower: usize,
    pub track: Option<usize>,
    pub upper: usize,
    pub weight: f64,
    pub x_lower: f64,
    pub x_upper: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LKind {
    Dummy,
    Exit {
        inst: usize,
        side: Side,
        to_self: bool,
    },
    Real(usize),
}

#[derive(Clone, Debug)]
pub struct LNode {
    pub down: Vec<usize>,
    pub h: f64,
    pub kind: LKind,
    pub prev_x: Option<f64>,
    pub rank: usize,
    pub up: Vec<usize>,
    pub w: f64,
    pub x: f64,
}

fn local_crossings(l: &Layered, r: usize, pos: &[usize]) -> f64 {
    let mut total = l.crossings_below(r, pos);
    if r > 0 {
        total += l.crossings_below(r - 1, pos);
    }
    return total;
}

pub fn reduce_crossings(l: &mut Layered, config: &LayoutConfig) {
    for r in 0 .. l.ranks.len() {
        let mut keyed: Vec<(f64, usize, usize)> =
            l.ranks[r]
                .iter()
                .enumerate()
                .map(|(i, n)| (l.nodes[*n].prev_x.unwrap_or(f64::MAX), i, *n))
                .collect();
        keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        l.ranks[r] = keyed.into_iter().map(|k| k.2).collect();
    }
    if l.ranks.len() <= 1 {
        return;
    }
    let mut best = l.ranks.clone();
    let mut best_score = l.total_crossings();
    for sweep in 0 .. config.order_sweeps {
        if best_score == 0. {
            break;
        }
        let down = sweep % 2 == 0;
        if down {
            for r in 1 .. l.ranks.len() {
                barycenter_sort(l, r, true);
            }
        } else {
            for r in (0 .. l.ranks.len() - 1).rev() {
                barycenter_sort(l, r, false);
            }
        }
        let score = l.total_crossings();
        if score < best_score {
            best_score = score;
            best = l.ranks.clone();
        }
    }
    l.ranks = best;
    for _ in 0 .. 20 {
        let mut improved = false;
        for r in 0 .. l.ranks.len() {
            if l.ranks[r].len() < 2 {
                continue;
            }
            let mut by_degree: Vec<usize> = l.ranks[r].clone();
            by_degree.sort_by_key(|n| (l.degree(*n), *n));
            for n in by_degree {
                let pos = l.positions();
                let i = pos[n];
                let before = local_crossings(l, r, &pos);
                for j in [i.wrapping_sub(1), i + 1] {
                    if j >= l.ranks[r].len() {
                        continue;
                    }
                    l.ranks[r].swap(i, j);
                    let pos2 = l.positions();
                    let after = local_crossings(l, r, &pos2);
                    if after < before - 1e-9 {
                        improved = true;
                        break;
                    }
                    l.ranks[r].swap(i, j);
                }
            }
        }
        if !improved {
            break;
        }
    }
}
