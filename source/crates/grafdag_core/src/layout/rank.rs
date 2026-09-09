#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_and_cycle() {
        let r = rank(4, &[(0, 1), (1, 2), (2, 0)], &[f64::MAX; 4]);
        assert_eq!(r.islands.len(), 2);
        assert_eq!(r.rank[0], 0);
        assert_eq!(r.rank[1], 1);
        assert_eq!(r.rank[2], 2);
        assert_eq!(r.n_ranks[r.island[0]], 3);
        assert_eq!(r.rank[3], 0);
    }

    #[test]
    fn source_pulled_down() {
        let r = rank(4, &[(0, 1), (1, 2), (3, 2)], &[f64::MAX; 4]);
        assert_eq!(r.rank[3], 1);
    }
}

use std::collections::HashSet;

pub fn rank(n: usize, edges: &[(usize, usize)], prev_order: &[f64]) -> Ranking {
    let mut uf: Vec<usize> = (0 .. n).collect();

    fn find(uf: &mut Vec<usize>, mut i: usize) -> usize {
        while uf[i] != i {
            uf[i] = uf[uf[i]];
            i = uf[i];
        }
        return i;
    }

    for (a, b) in edges {
        if a == b {
            continue;
        }
        let ra = find(&mut uf, *a);
        let rb = find(&mut uf, *b);
        if ra != rb {
            uf[ra] = rb;
        }
    }
    let mut roots: Vec<usize> = vec![];
    let mut island_of_root: Vec<Option<usize>> = vec![
        None;
        n
    ];
    let mut islands: Vec<Vec<usize>> = vec![];
    for i in 0 .. n {
        let r = find(&mut uf, i);
        let island = match island_of_root[r] {
            Some(x) => x,
            None => {
                let x = islands.len();
                island_of_root[r] = Some(x);
                islands.push(vec![]);
                roots.push(r);
                x
            },
        };
        islands[island].push(i);
    }
    let mut island_keys: Vec<(f64, usize, usize)> =
        islands
            .iter()
            .enumerate()
            .map(|(i, ms)| (ms.iter().map(|m| prev_order[*m]).fold(f64::MAX, f64::min), ms[0], i))
            .collect();
    island_keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    let islands: Vec<Vec<usize>> = island_keys.iter().map(|k| islands[k.2].clone()).collect();
    let mut island = vec![
        0;
        n
    ];
    for (i, ms) in islands.iter().enumerate() {
        for m in ms {
            island[*m] = i;
        }
    }
    let mut out_edges: Vec<Vec<usize>> = vec![
        vec![];
        n
    ];
    let mut in_degree = vec![
        0usize;
        n
    ];
    for (a, b) in edges {
        if a == b {
            continue;
        }
        out_edges[*a].push(*b);
        in_degree[*b] += 1;
    }
    let mut reversed: HashSet<(usize, usize)> = HashSet::new();
    let mut color = vec![
        0u8;
        n
    ];
    let mut start_order: Vec<usize> = (0 .. n).collect();
    start_order.sort_by_key(|i| (in_degree[*i] > 0, *i));
    for start in start_order {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        color[start] = 1;
        while let Some((node, child_i)) = stack.last().cloned() {
            if child_i >= out_edges[node].len() {
                color[node] = 2;
                stack.pop();
                continue;
            }
            stack.last_mut().unwrap().1 += 1;
            let child = out_edges[node][child_i];
            match color[child] {
                0 => {
                    color[child] = 1;
                    stack.push((child, 0));
                },
                1 => {
                    reversed.insert((node, child));
                },
                _ => { },
            }
        }
    }
    let mut dag_out: Vec<Vec<usize>> = vec![
        vec![];
        n
    ];
    let mut dag_in: Vec<Vec<usize>> = vec![
        vec![];
        n
    ];
    for (a, b) in edges {
        if a == b {
            continue;
        }
        let (a, b) = if reversed.contains(&(*a, *b)) {
            (*b, *a)
        } else {
            (*a, *b)
        };
        dag_out[a].push(b);
        dag_in[b].push(a);
    }
    let mut rank = vec![
        0usize;
        n
    ];
    let mut remaining: Vec<usize> = dag_in.iter().map(|x| x.len()).collect();
    let mut queue: Vec<usize> = (0 .. n).filter(|i| remaining[*i] == 0).collect();
    let mut topo = vec![];
    while !queue.is_empty() {
        queue.sort_unstable_by(|a, b| b.cmp(a));
        let node = queue.pop().unwrap();
        topo.push(node);
        for next in &dag_out[node] {
            rank[*next] = rank[*next].max(rank[node] + 1);
            remaining[*next] -= 1;
            if remaining[*next] == 0 {
                queue.push(*next);
            }
        }
    }
    for node in 0 .. n {
        if dag_in[node].is_empty() && !dag_out[node].is_empty() {
            let min_succ = dag_out[node].iter().map(|s| rank[*s]).min().unwrap();
            rank[node] = min_succ - 1;
        }
    }
    let mut n_ranks = vec![
        0usize;
        islands.len()
    ];
    for (i, ms) in islands.iter().enumerate() {
        let min = ms.iter().map(|m| rank[*m]).min().unwrap_or(0);
        for m in ms {
            rank[*m] -= min;
        }
        n_ranks[i] = ms.iter().map(|m| rank[*m]).max().unwrap_or(0) + 1;
    }
    return Ranking {
        island: island,
        rank: rank,
        islands: islands,
        n_ranks: n_ranks,
    };
}

#[derive(Clone, Debug)]
pub struct Ranking {
    pub island: Vec<usize>,
    pub islands: Vec<Vec<usize>>,
    pub n_ranks: Vec<usize>,
    pub rank: Vec<usize>,
}
