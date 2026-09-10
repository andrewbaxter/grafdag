use {
    crate::document::{
        Edge,
        Layer,
        Node,
    },
    super::*,
};

#[test]
fn all_flows() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("a", &[], &[]),
            node("b", &[], &[]),
            node("c", &[], &[]),
            node("g", &[], &[]),
            node("d", &["g"], &[]),
            node("e", &["g"], &[]),
            node("f", &[], &[]),
        ],
        edges: vec![
            edge("e1", "a", "b"),
            edge("e2", "a", "c"),
            edge("e3", "b", "d"),
            edge("e4", "d", "e"),
            edge("e5", "d", "g"),
            edge("e6", "e", "f"),
        ],
    };
    let mut previous: Option<Layout> = None;
    for flow in Flow::ALL {
        let mut config = LayoutConfig::default();
        config.flow = flow;
        let l = layout(&doc, &sizes(&doc), &no_titles(), &config, previous.as_ref());
        assert_eq!(l.flow, flow);
        check_no_sibling_overlap(&l);
        check_orthogonal(&l);
        let get = |id: &str| l.primary(&NodeId(id.into())).unwrap().clone();
        assert!(l.width > 0. && l.height > 0.);
        for n in &l.nodes {
            assert!(
                n.rect.x >= -0.01 && n.rect.y >= -0.01 && n.rect.right() <= l.width + 0.01 &&
                    n.rect.bottom() <= l.height + 0.01,
                "{:?} {:?} outside {}x{}",
                flow,
                n,
                l.width,
                l.height
            );
        }
        let (a, b, c) = (get("a"), get("b"), get("c"));
        let after = |p: &Rect, q: &Rect| match flow {
            Flow::Down => q.y >= p.bottom(),
            Flow::Up => q.bottom() <= p.y,
            Flow::Right => q.x >= p.right(),
            Flow::Left => q.right() <= p.x,
        };
        assert!(after(&a.rect, &b.rect), "{:?}: {:?} -> {:?}", flow, a.rect, b.rect);
        assert!(after(&a.rect, &c.rect), "{:?}: {:?} -> {:?}", flow, a.rect, c.rect);
        if flow.horizontal() {
            assert!((b.rect.cy() - c.rect.cy()).abs() > 10.);
        } else {
            assert!((b.rect.cx() - c.rect.cx()).abs() > 10.);
        }
        let e1 = l.edges.iter().find(|e| e.id.0 == "e1").unwrap();
        let first = e1.points[0];
        let last = *e1.points.last().unwrap();
        let on_side = |p: Pt, r: &Rect, side: ScreenDir| match side {
            ScreenDir::Up => (p.y - r.y).abs() < 0.01 && p.x >= r.x && p.x <= r.right(),
            ScreenDir::Down => (p.y - r.bottom()).abs() < 0.01 && p.x >= r.x && p.x <= r.right(),
            ScreenDir::Left => (p.x - r.x).abs() < 0.01 && p.y >= r.y && p.y <= r.bottom(),
            ScreenDir::Right => (p.x - r.right()).abs() < 0.01 && p.y >= r.y && p.y <= r.bottom(),
        };
        assert!(on_side(first, &a.rect, flow.screen_side(Side::After)), "{:?}: {:?} not on {:?}", flow, first, a.rect);
        assert!(on_side(last, &b.rect, flow.screen_side(Side::Before)), "{:?}: {:?} not on {:?}", flow, last, b.rect);
        assert_eq!(l.port(&e1.id, &a.id).unwrap().side, Side::After);
        assert_eq!(l.port(&e1.id, &b.id).unwrap().side, Side::Before);
        let pb = l.port(&e1.id, &a.id).unwrap().along;
        let pc = l.port(&EdgeId("e2".into()), &a.id).unwrap().along;
        let cb = l.canonical(pt(b.rect.cx(), b.rect.cy())).x;
        let cc = l.canonical(pt(c.rect.cx(), c.rect.cy())).x;
        assert_eq!(pb < pc, cb < cc, "{:?}", flow);
        let g = get("g");
        assert!(g.container);
        for child in ["d", "e"] {
            let r = get(child).rect;
            assert!(
                r.y >= g.rect.y + g.title_height - 0.01,
                "{:?}: {} {:?} above title of {:?}",
                flow,
                child,
                r,
                g.rect
            );
            assert!(
                r.x >= g.rect.x && r.right() <= g.rect.right() && r.bottom() <= g.rect.bottom(),
                "{:?}: {} {:?} outside {:?}",
                flow,
                child,
                r,
                g.rect
            );
        }
        let (d, e) = (get("d"), get("e"));
        assert!(after(&d.rect, &e.rect), "{:?}: {:?} -> {:?}", flow, d.rect, e.rect);
        let e5 = l.edges.iter().find(|e| e.id.0 == "e5").unwrap();
        let end = *e5.points.last().unwrap();
        assert!(
            (end.y - (g.rect.y + g.title_height)).abs() < 0.01,
            "{:?}: {:?} vs title bottom {}",
            flow,
            end,
            g.rect.y + g.title_height
        );
        assert!(end.x >= g.rect.x && end.x <= g.rect.right());
        for n in &l.nodes {
            let p = pt(n.rect.cx(), n.rect.cy());
            let back = flow.to_screen(l.extent(), l.canonical(p));
            assert!((back.x - p.x).abs() < 0.01 && (back.y - p.y).abs() < 0.01);
        }
        previous = Some(l);
    }
    for flow in Flow::ALL {
        for m in [Motion::Forward, Motion::Backward, Motion::SideNext, Motion::SidePrev] {
            assert_eq!(flow.motion(flow.screen_dir(m)), m);
        }
        let f = flow.screen_dir(Motion::Forward);
        let n = flow.screen_dir(Motion::SideNext);
        let vertical = |d: ScreenDir| matches!(d, ScreenDir::Up | ScreenDir::Down);
        assert_ne!(vertical(f), vertical(n), "{:?}: axes must be perpendicular", flow);
    }
    assert_eq!(Flow::Down.ccw(), Flow::Right);
    assert_eq!(Flow::Left.ccw(), Flow::Down);
    assert_eq!(Flow::Down.cw(), Flow::Left);
    assert_eq!(Flow::Right.cw(), Flow::Down);
}

fn check_no_sibling_overlap(l: &Layout) {
    for (i, a) in l.nodes.iter().enumerate() {
        for b in l.nodes.iter().skip(i + 1) {
            if a.id.container == b.id.container {
                let (r1, r2) = (&a.rect, &b.rect);
                let overlaps =
                    r1.x < r2.right() - 0.01 && r2.x < r1.right() - 0.01 && r1.y < r2.bottom() - 0.01 &&
                        r2.y < r1.bottom() - 0.01;
                assert!(!overlaps, "overlap {:?} {:?}", a, b);
            }
        }
    }
}

fn check_orthogonal(l: &Layout) {
    for e in &l.edges {
        assert!(e.points.len() >= 2, "edge {:?} has too few points", e.id);
        for w in e.points.windows(2) {
            let straight = (w[0].x - w[1].x).abs() < 0.01 || (w[0].y - w[1].y).abs() < 0.01;
            assert!(straight, "edge {:?} not orthogonal: {:?}", e.id, e.points);
        }
    }
}

fn edge(id: &str, s: &str, d: &str) -> Edge {
    return Edge {
        id: EdgeId(id.into()),
        text: "".into(),
        source: NodeId(s.into()),
        dest: NodeId(d.into()),
        layer: None,
    };
}

#[test]
fn ghost_of_container_uses_text_size() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("p", &[], &[]),
            node("q", &[], &[]),
            node("g", &["p", "q"], &[]),
            node("a", &["g"], &[]),
        ],
        edges: vec![edge("e1", "p", "q")],
    };
    let titles = [(NodeId("g".into()), NodeSize {
        width: 400.,
        height: 18.,
    })].into_iter().collect();
    let l = layout(&doc, &sizes(&doc), &titles, &LayoutConfig::default(), None);
    let g = l.primary(&NodeId("g".into())).unwrap();
    assert!(g.container && g.rect.w >= 400., "{:?}", g.rect);
    let ghost = l.nodes.iter().find(|n| n.id.node.0 == "g" && n.ghost).expect("ghost");
    assert!(!ghost.container);
    assert!(ghost.rect.w < 100., "ghost {:?} sized like the container", ghost.rect);
}

#[test]
fn ghosts_and_hidden_layers() {
    let doc = Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "L".into(),
            inactive: true,
        }],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("p", &[], &[]),
            node("q", &[], &[]),
            node("a", &["p", "q"], &[]),
            node("hidden", &[], &["L"]),
        ],
        edges: vec![edge("e1", "a", "p"), edge("e2", "hidden", "a")],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    assert!(l.primary(&NodeId("hidden".into())).is_none());
    let a_placements: Vec<&PlacedNode> = l.nodes.iter().filter(|n| n.id.node.0 == "a").collect();
    assert_eq!(a_placements.len(), 2);
    assert_eq!(a_placements.iter().filter(|n| n.ghost).count(), 1);
    assert_eq!(l.edges.iter().filter(|e| e.id.0 == "e1").count(), 2);
    assert_eq!(l.edges.iter().filter(|e| e.id.0 == "e2").count(), 0);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
}

#[test]
fn parents_through_hidden_layers() {
    let doc = Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "L".into(),
            inactive: true,
        }],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("g", &[], &[]),
            node("mid", &["g"], &["L"]),
            node("a", &["mid"], &[]),
            node("b", &["mid"], &[]),
        ],
        edges: vec![edge("e1", "a", "b")],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    assert!(l.primary(&NodeId("mid".into())).is_none());
    let g = l.primary(&NodeId("g".into())).unwrap();
    assert!(g.container);
    for id in ["a", "b"] {
        let n = l.primary(&NodeId(id.into())).unwrap();
        assert_eq!(n.id.container, Some(NodeId("g".into())), "{} not contained by g", id);
        assert_eq!(n.depth, 1);
    }
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
}

#[test]
fn islands_and_cycle() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("a", &[], &[]),
            node("b", &[], &[]),
            node("c", &[], &[]),
            node("x", &[], &[]),
            node("y", &[], &[]),
        ],
        edges: vec![edge("e1", "a", "b"), edge("e2", "b", "c"), edge("e3", "c", "a"), edge("e4", "x", "y")],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    assert_eq!(l.islands.len(), 2);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let e3 = l.edges.iter().find(|e| e.id.0 == "e3").unwrap();
    assert!(e3.reversed);
    let c = l.primary(&NodeId("c".into())).unwrap();
    assert!((e3.points[0].y - c.rect.y).abs() < 0.01);
}

#[test]
fn narrow_ranks_not_split() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("a", &[], &[]),
            node("b", &[], &[]),
            node("c", &[], &[]),
            node("g", &[], &[]),
            node("d", &["g"], &[]),
        ],
        edges: vec![edge("e1", "a", "b"), edge("e2", "a", "c"), edge("e3", "b", "d")],
    };
    let mut config = LayoutConfig::default();
    config.max_rank_width = Some(720.);
    let l = layout(&doc, &sizes(&doc), &no_titles(), &config, None);
    assert_eq!(l.islands[0].ranks.len(), 3, "{:?}", l.islands[0].ranks);
}

#[test]
fn nested_containers() {
    let doc = Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "L".into(),
            inactive: false,
        }],
        selected_layer: Some(LayerId("L".into())),
        flow: Default::default(),
        nodes: vec![
            node("g", &[], &["L"]),
            node("a", &["g"], &["L"]),
            node("b", &["g"], &[]),
            node("c", &[], &["L"]),
            node("d", &[], &[]),
            node("h", &["g"], &[]),
            node("i", &["h"], &[]),
        ],
        edges: vec![
            edge("e1", "a", "b"),
            edge("e2", "b", "c"),
            edge("e3", "d", "a"),
            edge("e4", "c", "d"),
            edge("e5", "i", "b"),
            edge("e6", "i", "c"),
            edge("e7", "g", "a"),
            edge("e8", "a", "a"),
        ],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let g = l.primary(&NodeId("g".into())).unwrap();
    assert!(g.container);
    for id in ["a", "b", "h"] {
        let n = l.primary(&NodeId(id.into())).unwrap();
        assert_eq!(n.depth, 1);
        assert!(n.rect.x >= g.rect.x && n.rect.right() <= g.rect.right(), "{} not inside g horizontally", id);
        assert!(
            n.rect.y >= g.rect.y + g.title_height && n.rect.bottom() <= g.rect.bottom(),
            "{} not inside g vertically",
            id
        );
    }
    let i = l.primary(&NodeId("i".into())).unwrap();
    assert_eq!(i.depth, 2);
    assert_eq!(l.edges.len(), 8);
    let e2 = l.edges.iter().find(|e| e.id.0 == "e2").unwrap();
    let b = l.primary(&NodeId("b".into())).unwrap();
    let c = l.primary(&NodeId("c".into())).unwrap();
    assert!((e2.points[0].y - b.rect.bottom()).abs() < 0.01, "{:?}", e2.points);
    assert!((e2.points.last().unwrap().y - c.rect.y).abs() < 0.01, "{:?}", e2.points);
    let e6 = l.edges.iter().find(|e| e.id.0 == "e6").unwrap();
    assert!((e6.points[0].y - i.rect.bottom()).abs() < 0.01, "{:?}", e6.points);
    assert!((e6.points.last().unwrap().y - c.rect.y).abs() < 0.01, "{:?}", e6.points);
}

fn no_titles() -> HashMap<NodeId, NodeSize> {
    return HashMap::new();
}

fn node(id: &str, parents: &[&str], layers: &[&str]) -> Node {
    return Node {
        id: NodeId(id.into()),
        text: id.into(),
        layers: layers.iter().map(|l| LayerId(l.to_string())).collect(),
        parents: parents.iter().map(|p| NodeId(p.to_string())).collect(),
    };
}

#[test]
fn ports_follow_drawn_order() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![node("p", &[], &[]), node("b", &[], &[]), node("c1", &[], &[]), node("c2", &[], &[])],
        edges: vec![edge("in", "p", "b"), edge("x", "b", "c1"), edge("y1", "b", "c2"), edge("y2", "b", "c2")],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    let b = l.primary(&NodeId("b".into())).unwrap().clone();
    let port = |e: &str| l.port(&EdgeId(e.into()), &b.id).unwrap();
    assert_eq!(port("in").side, Side::Before);
    for e in ["x", "y1", "y2"] {
        let p = port(e);
        assert_eq!(p.side, Side::After);
        assert!(p.along >= b.rect.x && p.along <= b.rect.right());
    }
    let c1 = l.primary(&NodeId("c1".into())).unwrap().rect.x;
    let c2 = l.primary(&NodeId("c2".into())).unwrap().rect.x;
    let (x, y1, y2) = (port("x").along, port("y1").along, port("y2").along);
    assert!(y1 < y2);
    if c1 < c2 {
        assert!(x < y1);
    } else {
        assert!(y2 < x);
    }
    assert!(l.port(&EdgeId("nope".into()), &b.id).is_none());
}

#[test]
fn ports_widen_container() {
    let mut nodes = vec![node("g", &[], &[]), node("a", &["g"], &[])];
    let mut edges = vec![];
    let n = 10;
    for i in 0 .. n {
        nodes.push(node(&format!("s{}", i), &[], &[]));
        edges.push(edge(&format!("e{}", i), &format!("s{}", i), "a"));
    }
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: nodes,
        edges: edges,
    };
    let config = LayoutConfig::default();
    let l = layout(&doc, &sizes(&doc), &no_titles(), &config, None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let g = l.primary(&NodeId("g".into())).unwrap();
    let a = l.primary(&NodeId("a".into())).unwrap();
    let min_w = 2. * config.port_margin + (n as f64 - 1.) * config.port_gap;
    assert!(g.rect.w >= min_w - 0.01, "box {:?} too narrow for {} ports (want {})", g.rect, n, min_w);
    assert!((a.rect.cx() - g.rect.cx()).abs() < 0.01, "child {:?} not centered in {:?}", a.rect, g.rect);
    for e in &l.edges {
        let cross =
            e
                .points
                .windows(2)
                .find(|w| w[0].y <= g.rect.y + 0.01 && w[1].y >= g.rect.y - 0.01)
                .unwrap_or_else(|| panic!("{:?} doesn't enter g {:?}: {:?}", e.id, g.rect, e.points));
        assert!((cross[0].x - cross[1].x).abs() < 0.01, "{:?} crosses g's border sideways: {:?}", e.id, cross);
        assert!(
            cross[0].x >= g.rect.x - 0.01 && cross[0].x <= g.rect.right() + 0.01,
            "{:?} enters g outside its box: {:?} {:?}",
            e.id,
            cross,
            g.rect
        );
    }
}

#[test]
fn simple_chain() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![node("a", &[], &[]), node("b", &[], &[]), node("c", &[], &[])],
        edges: vec![edge("e1", "a", "b"), edge("e2", "b", "c"), edge("e3", "a", "c")],
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    assert_eq!(l.nodes.len(), 3);
    assert_eq!(l.edges.len(), 3);
    let a = l.primary(&NodeId("a".into())).unwrap();
    let b = l.primary(&NodeId("b".into())).unwrap();
    let c = l.primary(&NodeId("c".into())).unwrap();
    assert!(a.rect.y < b.rect.y && b.rect.y < c.rect.y);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let e3 = l.edges.iter().find(|e| e.id.0 == "e3").unwrap();
    assert!((e3.points[0].y - a.rect.bottom()).abs() < 0.01);
    assert!((e3.points.last().unwrap().y - c.rect.y).abs() < 0.01);
    assert_eq!(l.islands.len(), 1);
}

fn sizes(doc: &Document) -> HashMap<NodeId, NodeSize> {
    return doc.nodes.iter().map(|n| (n.id.clone(), NodeSize {
        width: 60.,
        height: 24.,
    })).collect();
}

#[test]
fn title_strip_fits_text() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![node("g", &[], &[]), node("a", &["g"], &[]), node("x", &[], &[])],
        edges: vec![edge("e1", "x", "a")],
    };
    let title = NodeSize {
        width: 200.,
        height: 20.,
    };
    let mut sizes = sizes(&doc);
    sizes.insert(NodeId("g".into()), title);
    sizes.insert(NodeId("a".into()), NodeSize {
        width: 20.,
        height: 20.,
    });
    for flow in Flow::ALL {
        let mut config = LayoutConfig::default();
        config.flow = flow;
        let l = layout(&doc, &sizes, &no_titles(), &config, None);
        let g = l.primary(&NodeId("g".into())).unwrap();
        assert!(g.rect.w >= title.width - 0.01, "{:?}: title {:?} wider than box {:?}", flow, title, g.rect);
        assert!(
            g.title_height >= title.height - 0.01,
            "{:?}: title strip {} thinner than {:?}",
            flow,
            g.title_height,
            title
        );
        assert!(g.title_height <= title.height + 0.01, "{:?}: title strip {} too thick", flow, g.title_height);
        let a = l.primary(&NodeId("a".into())).unwrap();
        assert!(
            a.rect.y >= g.rect.y + g.title_height - 0.01,
            "{:?}: child {:?} in title of {:?}",
            flow,
            a.rect,
            g.rect
        );
        let wide = NodeSize {
            width: 400.,
            height: 18.,
        };
        let l = layout(&doc, &sizes, &[(NodeId("g".into()), wide)].into_iter().collect(), &config, None);
        let g = l.primary(&NodeId("g".into())).unwrap();
        assert!(g.rect.w >= wide.width - 0.01, "{:?}: box {:?} narrower than title {:?}", flow, g.rect, wide);
        assert!(
            (g.title_height - wide.height).abs() < 0.01,
            "{:?}: strip {} for title {:?}",
            flow,
            g.title_height,
            wide
        );
    }
}

#[test]
fn wide_graph_no_overlap() {
    let mut nodes = vec![];
    let mut edges = vec![];
    for i in 0 .. 12 {
        nodes.push(node(&format!("n{}", i), &[], &[]));
    }
    let pairs =
        [
            (0, 1),
            (0, 2),
            (0, 3),
            (1, 4),
            (2, 4),
            (3, 5),
            (4, 6),
            (5, 6),
            (1, 7),
            (7, 6),
            (8, 9),
            (9, 10),
            (8, 10),
            (10, 11),
            (0, 11),
            (2, 5),
        ];
    for (i, (s, d)) in pairs.iter().enumerate() {
        edges.push(edge(&format!("e{}", i), &format!("n{}", s), &format!("n{}", d)));
    }
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: nodes,
        edges: edges,
    };
    let l = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let l2 = layout(&doc, &sizes(&doc), &no_titles(), &LayoutConfig::default(), Some(&l));
    for n in &l.nodes {
        let n2 = l2.node(&n.id).unwrap();
        assert_eq!(n.nav, n2.nav);
    }
}

#[test]
fn wide_ranks_are_split() {
    let mut nodes = vec![node("root", &[], &[])];
    let mut edges = vec![];
    for i in 0 .. 10 {
        let id = format!("c{}", i);
        nodes.push(node(&id, &[], &[]));
        edges.push(edge(&format!("e{}", i), "root", &id));
    }
    nodes.push(node("sink", &[], &[]));
    edges.push(edge("es", "c0", "sink"));
    edges.push(edge("es2", "c9", "sink"));
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        flow: Default::default(),
        nodes: nodes,
        edges: edges,
    };
    let mut config = LayoutConfig::default();
    config.max_rank_width = Some(300.);
    let l = layout(&doc, &sizes(&doc), &no_titles(), &config, None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    assert!(l.width < 500., "width {}", l.width);
    let ranks = &l.islands[0].ranks;
    assert!(ranks.len() >= 5, "ranks {}", ranks.len());
    for r in ranks {
        assert!(r.len() <= 4);
    }
    let root = l.primary(&NodeId("root".into())).unwrap();
    let sink = l.primary(&NodeId("sink".into())).unwrap();
    for i in 0 .. 10 {
        let c = l.primary(&NodeId(format!("c{}", i))).unwrap();
        assert!(c.rect.y > root.rect.y && c.rect.y < sink.rect.y);
    }
    assert_eq!(l.edges.len(), 12);
    let l2 = layout(&doc, &sizes(&doc), &no_titles(), &config, Some(&l));
    let l3 = layout(&doc, &sizes(&doc), &no_titles(), &config, Some(&l2));
    for n in &l2.nodes {
        assert_eq!(n.nav, l3.node(&n.id).unwrap().nav);
    }
}

#[test]
fn links_never_share_an_end_point() {
    // Assorted small graphs, including links to and from container nodes, where
    // ports are easy to collapse onto one point.
    let mut seed: u64 = 12345;
    let mut rnd = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        return (seed >> 33) as usize;
    };
    for trial in 0 .. 400 {
        let n_groups = rnd() % 4;
        let n_nodes = 4 + rnd() % 8;
        let mut nodes = vec![];
        for g in 0 .. n_groups {
            let parent = if g > 0 && rnd() % 3 == 0 {
                vec![format!("g{}", rnd() % g)]
            } else {
                vec![]
            };
            nodes.push(node(&format!("g{}", g), &parent.iter().map(|p| p.as_str()).collect::<Vec<_>>(), &[]));
        }
        for i in 0 .. n_nodes {
            let parent = if n_groups > 0 && rnd() % 2 == 0 {
                vec![format!("g{}", rnd() % n_groups)]
            } else {
                vec![]
            };
            nodes.push(node(&format!("n{}", i), &parent.iter().map(|p| p.as_str()).collect::<Vec<_>>(), &[]));
        }
        let mut edges = vec![];
        for e in 0 .. n_nodes + rnd() % (2 * n_nodes) {
            let mut end = || {
                let i = rnd() % n_nodes;
                if n_groups > 0 && rnd() % 4 == 0 {
                    return format!("g{}", i % n_groups);
                } else {
                    return format!("n{}", i);
                }
            };
            let (source, dest) = (end(), end());
            if source == dest {
                continue;
            }
            edges.push(edge(&format!("e{}", e), &source, &dest));
        }
        let doc = Document {
            layers: vec![],
            selected_layer: None,
            flow: Default::default(),
            nodes: nodes,
            edges: edges,
        };
        for flow in Flow::ALL {
            let mut config = LayoutConfig::default();
            config.flow = flow;
            let l = layout(&doc, &sizes(&doc), &no_titles(), &config, None);
            check_no_sibling_overlap(&l);
            let mut ends: HashMap<(PlacementId, i64, i64), Vec<&EdgeId>> = HashMap::new();
            for e in &l.edges {
                for (at, p) in [(&e.source, e.points.first()), (&e.dest, e.points.last())] {
                    let Some(p) = p else {
                        continue;
                    };
                    ends
                        .entry((at.clone(), (p.x * 10.).round() as i64, (p.y * 10.).round() as i64))
                        .or_default()
                        .push(&e.id);
                }
            }
            for ((at, x, y), sharing) in &ends {
                assert!(
                    sharing.len() == 1,
                    "trial {} {:?}: {:?} all meet {:?} at {},{}",
                    trial,
                    flow,
                    sharing,
                    at,
                    *x as f64 / 10.,
                    *y as f64 / 10.
                );
            }
        }
    }
}
