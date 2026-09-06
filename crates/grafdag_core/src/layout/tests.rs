use {
    super::*,
    crate::document::{
        Edge,
        Layer,
        Node,
    },
};

fn node(id: &str, parents: &[&str], layers: &[&str]) -> Node {
    return Node {
        id: NodeId(id.into()),
        text: id.into(),
        layers: layers.iter().map(|l| LayerId(l.to_string())).collect(),
        parents: parents.iter().map(|p| NodeId(p.to_string())).collect(),
    };
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

fn sizes(doc: &Document) -> HashMap<NodeId, NodeSize> {
    return doc.nodes.iter().map(|n| (n.id.clone(), NodeSize {
        width: 60.,
        height: 24.,
    })).collect();
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    return a.x < b.right() - 0.01 && b.x < a.right() - 0.01 && a.y < b.bottom() - 0.01 && b.y < a.bottom() - 0.01;
}

fn check_no_sibling_overlap(l: &Layout) {
    for (i, a) in l.nodes.iter().enumerate() {
        for b in l.nodes.iter().skip(i + 1) {
            if a.id.container == b.id.container {
                assert!(!overlaps(&a.rect, &b.rect), "overlap {:?} {:?}", a, b);
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

#[test]
fn simple_chain() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        nodes: vec![node("a", &[], &[]), node("b", &[], &[]), node("c", &[], &[])],
        edges: vec![edge("e1", "a", "b"), edge("e2", "b", "c"), edge("e3", "a", "c")],
    };
    let l = layout(&doc, &sizes(&doc), &LayoutConfig::default(), None);
    assert_eq!(l.nodes.len(), 3);
    assert_eq!(l.edges.len(), 3);
    let a = l.primary(&NodeId("a".into())).unwrap();
    let b = l.primary(&NodeId("b".into())).unwrap();
    let c = l.primary(&NodeId("c".into())).unwrap();
    assert!(a.rect.y < b.rect.y && b.rect.y < c.rect.y);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    // The long edge a->c starts at a's bottom and ends at c's top
    let e3 = l.edges.iter().find(|e| e.id.0 == "e3").unwrap();
    assert!((e3.points[0].y - a.rect.bottom()).abs() < 0.01);
    assert!((e3.points.last().unwrap().y - c.rect.y).abs() < 0.01);
    assert_eq!(l.islands.len(), 1);
}

#[test]
fn islands_and_cycle() {
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        nodes: vec![node("a", &[], &[]), node("b", &[], &[]), node("c", &[], &[]), node("x", &[], &[]), node("y", &[], &[])],
        edges: vec![edge("e1", "a", "b"), edge("e2", "b", "c"), edge("e3", "c", "a"), edge("e4", "x", "y")],
    };
    let l = layout(&doc, &sizes(&doc), &LayoutConfig::default(), None);
    assert_eq!(l.islands.len(), 2);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let e3 = l.edges.iter().find(|e| e.id.0 == "e3").unwrap();
    assert!(e3.reversed);
    let c = l.primary(&NodeId("c".into())).unwrap();
    // Reversed edge leaves c's top
    assert!((e3.points[0].y - c.rect.y).abs() < 0.01);
}

#[test]
fn nested_containers() {
    let doc = Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "L".into(),
            active: true,
        }],
        selected_layer: Some(LayerId("L".into())),
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
    let l = layout(&doc, &sizes(&doc), &LayoutConfig::default(), None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    let g = l.primary(&NodeId("g".into())).unwrap();
    assert!(g.container);
    for id in ["a", "b", "h"] {
        let n = l.primary(&NodeId(id.into())).unwrap();
        assert_eq!(n.depth, 1);
        assert!(n.rect.x >= g.rect.x && n.rect.right() <= g.rect.right(), "{} not inside g horizontally", id);
        assert!(n.rect.y >= g.rect.y + g.title_height && n.rect.bottom() <= g.rect.bottom(), "{} not inside g vertically", id);
    }
    let i = l.primary(&NodeId("i".into())).unwrap();
    assert_eq!(i.depth, 2);
    assert_eq!(l.edges.len(), 8);
    // Edge from inside g to outside ends at c's top
    let e2 = l.edges.iter().find(|e| e.id.0 == "e2").unwrap();
    let b = l.primary(&NodeId("b".into())).unwrap();
    let c = l.primary(&NodeId("c".into())).unwrap();
    assert!((e2.points[0].y - b.rect.bottom()).abs() < 0.01, "{:?}", e2.points);
    assert!((e2.points.last().unwrap().y - c.rect.y).abs() < 0.01, "{:?}", e2.points);
    // Deep edge i -> c crosses two borders
    let e6 = l.edges.iter().find(|e| e.id.0 == "e6").unwrap();
    assert!((e6.points[0].y - i.rect.bottom()).abs() < 0.01, "{:?}", e6.points);
    assert!((e6.points.last().unwrap().y - c.rect.y).abs() < 0.01, "{:?}", e6.points);
}

#[test]
fn ghosts_and_hidden_layers() {
    let doc = Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "L".into(),
            active: false,
        }],
        selected_layer: None,
        nodes: vec![
            node("p", &[], &[]),
            node("q", &[], &[]),
            node("a", &["p", "q"], &[]),
            node("hidden", &[], &["L"]),
        ],
        edges: vec![edge("e1", "a", "p"), edge("e2", "hidden", "a")],
    };
    let l = layout(&doc, &sizes(&doc), &LayoutConfig::default(), None);
    assert!(l.primary(&NodeId("hidden".into())).is_none());
    let a_placements: Vec<&PlacedNode> = l.nodes.iter().filter(|n| n.id.node.0 == "a").collect();
    assert_eq!(a_placements.len(), 2);
    assert_eq!(a_placements.iter().filter(|n| n.ghost).count(), 1);
    // Edge e1 is drawn for the primary and the ghost
    assert_eq!(l.edges.iter().filter(|e| e.id.0 == "e1").count(), 2);
    assert_eq!(l.edges.iter().filter(|e| e.id.0 == "e2").count(), 0);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
}

#[test]
fn wide_graph_no_overlap() {
    let mut nodes = vec![];
    let mut edges = vec![];
    for i in 0 .. 12 {
        nodes.push(node(&format!("n{}", i), &[], &[]));
    }
    let pairs = [(0, 1), (0, 2), (0, 3), (1, 4), (2, 4), (3, 5), (4, 6), (5, 6), (1, 7), (7, 6), (8, 9), (9, 10), (8, 10), (10, 11), (0, 11), (2, 5)];
    for (i, (s, d)) in pairs.iter().enumerate() {
        edges.push(edge(&format!("e{}", i), &format!("n{}", s), &format!("n{}", d)));
    }
    let doc = Document {
        layers: vec![],
        selected_layer: None,
        nodes: nodes,
        edges: edges,
    };
    let l = layout(&doc, &sizes(&doc), &LayoutConfig::default(), None);
    check_no_sibling_overlap(&l);
    check_orthogonal(&l);
    // Stable relayout
    let l2 = layout(&doc, &sizes(&doc), &LayoutConfig::default(), Some(&l));
    for n in &l.nodes {
        let n2 = l2.node(&n.id).unwrap();
        assert_eq!(n.nav, n2.nav);
    }
}
