//! In-browser tests of the widget: layout, keyboard/mouse interaction, editing
//! and undo. Run with `cargo test -p grafdag_web --target wasm32-unknown-unknown`
//! (needs wasm-bindgen-test-runner and chromedriver).
use {
    gloo_utils::document,
    grafdag_core::{
        Document,
        Edge,
        EdgeId,
        Layer,
        LayerId,
        Node,
        NodeId,
    },
    grafdag_web::widget::{
        render::WORLD_MARGIN,
        state::Mode,
        Widget,
    },
    lunk::EventGraph,
    rooting::{
        el,
        set_root_non_dom,
    },
    std::{
        cell::RefCell,
        rc::Rc,
    },
    wasm_bindgen::JsCast,
    wasm_bindgen_test::*,
    web_sys::{
        Element,
        Event,
        EventInit,
        HtmlElement,
        HtmlInputElement,
        HtmlTextAreaElement,
        KeyboardEvent,
        KeyboardEventInit,
        MouseEvent,
        MouseEventInit,
        WheelEvent,
        WheelEventInit,
    },
};

wasm_bindgen_test_configure!(run_in_browser);

fn node(id: &str, text: &str, parents: &[&str], layers: &[&str]) -> Node {
    return Node {
        id: NodeId(id.into()),
        text: text.into(),
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

fn sample_doc() -> Document {
    return Document {
        layers: vec![Layer {
            id: LayerId("L".into()),
            name: "Layer L".into(),
            active: true,
        }],
        selected_layer: None,
        flow: Default::default(),
        nodes: vec![
            node("a", "Alpha", &[], &[]),
            node("b", "Beta", &[], &[]),
            node("c", "Gamma", &[], &["L"]),
            node("g", "Group", &[], &[]),
            node("d", "Delta", &["g"], &[]),
        ],
        edges: vec![edge("e1", "a", "b"), edge("e2", "a", "c"), edge("e3", "b", "d")],
    };
}

struct Harness {
    widget: Widget,
    saved: Rc<RefCell<Vec<Document>>>,
}

/// The app stylesheet, so sizes, hit areas and colors are real.
fn ensure_stylesheet() {
    if document().query_selector("#gd_test_style").unwrap().is_some() {
        return;
    }
    let style = document().create_element("style").unwrap();
    style.set_id("gd_test_style");
    style.set_text_content(Some(include_str!("../../../static/style.css")));
    document().head().unwrap().append_child(&style).unwrap();
}

fn setup(doc: Document) -> Harness {
    ensure_stylesheet();
    let saved: Rc<RefCell<Vec<Document>>> = Rc::new(RefCell::new(vec![]));
    let eg = EventGraph::new();
    let widget = Widget::new(&eg, doc, Box::new({
        let saved = saved.clone();
        move |d| saved.borrow_mut().push(d.clone())
    }));
    // Mount inside a host element rather than replacing the body (the test
    // harness lives there too)
    if let Some(old) = document().query_selector(".gd_test_host").unwrap() {
        old.remove();
    }
    let host = el("div").classes(&["gd_test_host"]).push(widget.el().clone());
    document().body().unwrap().append_child(&host.raw()).unwrap();
    set_root_non_dom(host);
    // Transitions would make assertions time dependent
    widget.state().animate.set(false);
    widget.refresh();
    return Harness {
        widget: widget,
        saved: saved,
    };
}

fn key(k: &str, shift: bool) {
    let init = KeyboardEventInit::new();
    init.set_key(k);
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_shift_key(shift);
    let ev = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    document().dispatch_event(&ev).unwrap();
}

fn node_el(id: &str) -> Element {
    return document().query_selector(&format!(".gd_node_wrap[data-node=\"{}\"]", id)).unwrap().expect("node element");
}

fn mouse(el: &Element, button: i16) {
    let init = MouseEventInit::new();
    init.set_button(button);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let ev = MouseEvent::new_with_mouse_event_init_dict("mousedown", &init).unwrap();
    el.dispatch_event(&ev).unwrap();
}

fn sel(h: &Harness) -> (Option<String>, Option<String>) {
    let s = h.widget.state();
    return (s.sel_start.get().map(|x| x.0), s.sel_end.get().map(|x| x.0));
}

#[wasm_bindgen_test]
fn renders_and_measures() {
    let h = setup(sample_doc());
    let layout = h.widget.state().layout.borrow().clone();
    assert_eq!(layout.nodes.len(), 5);
    for n in &layout.nodes {
        assert!(n.rect.w > 20. && n.rect.h > 10., "node {:?} has no size: {:?}", n.id, n.rect);
        assert!(n.rect.w < 300., "node {:?} is too wide: {:?}", n.id, n.rect);
    }
    let g = layout.primary(&NodeId("g".into())).unwrap();
    let d = layout.primary(&NodeId("d".into())).unwrap();
    assert!(g.container);
    assert!(d.rect.x > g.rect.x && d.rect.y > g.rect.y + g.title_height);
    assert_eq!(layout.edges.len(), 3);
    assert_eq!(document().query_selector_all(".gd_edge").unwrap().length(), 3);
    assert_eq!(node_el("c").get_attribute("data-layer-color"), Some("0".into()));
    // Alpha's text box is one line
    let a = node_el("a").first_element_child().unwrap().dyn_into::<HtmlElement>().unwrap();
    assert!(a.offset_height() < 40, "{}", a.offset_height());
}

#[wasm_bindgen_test]
fn keyboard_navigation() {
    let h = setup(sample_doc());
    key("PageDown", false);
    assert_eq!(sel(&h), (Some("a".into()), None));
    key("ArrowDown", false);
    let (s, e) = sel(&h);
    assert_eq!(s, Some("a".into()));
    assert!(e == Some("b".into()) || e == Some("c".into()));
    // Cycle the selected link among the start node's links; the end follows
    key("ArrowRight", false);
    let (_, e2) = sel(&h);
    assert!(e2.is_some() && e2 != e);
    let edge = h.widget.state().sel_edge.get().expect("link selected");
    assert!(document().query_selector(&format!(".gd_edge_selected[data-edge=\"{}\"]", edge.0)).unwrap().is_some());
    // Shift+Down cycles the end among the start's successors
    key("ArrowDown", true);
    let (_, e3) = sel(&h);
    assert!(e3 == Some("b".into()) || e3 == Some("c".into()));
    // Move forward: start becomes end
    key("ArrowDown", false);
    let (s4, e4) = sel(&h);
    assert_eq!(Some(s4.clone().unwrap()), e3);
    // Select "a" (above) as the end: the selection now points up, so Down swaps
    if e4.is_none() {
        mouse(&node_el("a"), 2);
        assert_eq!(sel(&h), (s4.clone(), Some("a".into())));
        key("ArrowDown", false);
        assert_eq!(sel(&h), (Some("a".into()), s4.clone()));
        // Now it points down, so Up swaps back
        key("ArrowUp", false);
        assert_eq!(sel(&h), (s4.clone(), Some("a".into())));
    }
    // Escape clears end, then start
    key("Escape", false);
    key("Escape", false);
    assert_eq!(sel(&h), (None, None));
    // Tab toggles the side panel
    assert!(h.widget.state().panel_open.get());
    key("Tab", false);
    assert!(!h.widget.state().panel_open.get());
}

#[wasm_bindgen_test]
fn mouse_selection_and_zoom() {
    let h = setup(sample_doc());
    mouse(&node_el("a"), 0);
    assert_eq!(sel(&h), (Some("a".into()), None));
    // Clicking a node in another layer makes that its first layer current
    assert_eq!(h.widget.state().doc.borrow().selected_layer, None);
    mouse(&node_el("c"), 0);
    assert_eq!(h.widget.state().doc.borrow().selected_layer, Some(LayerId("L".into())));
    mouse(&node_el("a"), 0);
    assert_eq!(h.widget.state().doc.borrow().selected_layer, Some(LayerId("L".into())));
    mouse(&node_el("b"), 2);
    assert_eq!(sel(&h), (Some("a".into()), Some("b".into())));
    assert!(node_el("a").class_list().contains("gd_node_start"));
    assert!(node_el("b").class_list().contains("gd_node_end"));
    assert_eq!(h.widget.state().sel_edge.get(), Some(EdgeId("e1".into())));
    assert!(document().query_selector(".gd_edge_selected[data-edge=\"e1\"]").unwrap().is_some());
    // Selected nodes and their edges are unfaded
    assert!(node_el("a").class_list().contains("gd_active"));
    let opacity = |id: &str| node_el(id).dyn_into::<HtmlElement>().unwrap().style().get_property_value("opacity").unwrap().parse::<f64>().unwrap();
    assert_eq!(opacity("a"), 1.);
    assert_eq!(opacity("c"), 0.6);
    assert!(!node_el("c").class_list().contains("gd_active"));
    assert!(document().query_selector(".gd_edge.gd_active").unwrap().is_some());
    // Hovering another node unfades it in addition to the selection
    let hover_init = MouseEventInit::new();
    hover_init.set_bubbles(false);
    node_el("c").dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseenter", &hover_init).unwrap()).unwrap();
    assert!(node_el("c").class_list().contains("gd_active"));
    assert!(node_el("a").class_list().contains("gd_active"));
    node_el("c").dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseleave", &hover_init).unwrap()).unwrap();
    assert!(!node_el("c").class_list().contains("gd_active"));
    assert!(node_el("a").class_list().contains("gd_active"));
    // Hovering a link unfades it and its ends
    let hit = document().query_selector(".gd_edge_hit[data-edge=\"e2\"]").unwrap().unwrap();
    hit.dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseenter", &hover_init).unwrap()).unwrap();
    assert!(document().query_selector(".gd_edge.gd_active[data-edge=\"e2\"]").unwrap().is_some());
    assert!(node_el("c").class_list().contains("gd_active"));
    hit.dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseleave", &hover_init).unwrap()).unwrap();
    assert!(!node_el("c").class_list().contains("gd_active"));
    // Toggle off
    mouse(&node_el("b"), 2);
    assert_eq!(sel(&h), (Some("a".into()), None));
    assert_eq!(h.widget.state().sel_edge.get(), None);
    // Clicking a link selects its ends by direction
    let path = document().query_selector(".gd_edge_hit[data-edge=\"e2\"]").unwrap().unwrap();
    mouse(&path, 0);
    assert_eq!(sel(&h), (Some("a".into()), Some("c".into())));
    assert_eq!(h.widget.state().sel_edge.get(), Some(EdgeId("e2".into())));
    // Wheel zooms
    let canvas = document().query_selector(".gd_canvas").unwrap().unwrap();
    let z0 = h.widget.state().zoom.get();
    let init = WheelEventInit::new();
    init.set_delta_y(-100.);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let ev = WheelEvent::new_with_event_init_dict("wheel", &init).unwrap();
    canvas.dispatch_event(&ev).unwrap();
    assert!(h.widget.state().zoom.get() > z0);
}

#[wasm_bindgen_test]
fn link_unlink_reverse_delete_undo() {
    let h = setup(sample_doc());
    mouse(&node_el("b"), 0);
    mouse(&node_el("c"), 2);
    key("l", false);
    {
        let doc = h.widget.state().doc.borrow();
        assert!(doc.edges.iter().any(|e| e.source.0 == "b" && e.dest.0 == "c"));
    }
    assert_eq!(document().query_selector_all(".gd_edge").unwrap().length(), 4);
    key("r", false);
    {
        let doc = h.widget.state().doc.borrow();
        assert!(doc.edges.iter().any(|e| e.source.0 == "c" && e.dest.0 == "b"));
    }
    key("u", false);
    assert_eq!(h.widget.state().doc.borrow().edges.len(), 3);
    // Delete the end node
    key("Delete", false);
    assert!(h.widget.state().doc.borrow().node(&NodeId("c".into())).is_none());
    assert_eq!(sel(&h), (Some("b".into()), None));
    assert_eq!(document().query_selector_all(".gd_node_wrap").unwrap().length(), 4);
    // Undo restores it, redo deletes again
    key("z", false);
    assert!(h.widget.state().doc.borrow().node(&NodeId("c".into())).is_some());
    assert_eq!(document().query_selector_all(".gd_node_wrap").unwrap().length(), 5);
    key("Z", false);
    assert!(h.widget.state().doc.borrow().node(&NodeId("c".into())).is_none());
    // Every change notified
    assert!(h.saved.borrow().len() >= 6);
}

#[wasm_bindgen_test]
fn create_and_edit_node() {
    let h = setup(sample_doc());
    mouse(&node_el("a"), 0);
    key("n", false);
    let (s, e) = sel(&h);
    assert_eq!(s, Some("a".into()));
    let new_id = e.expect("new node selected as end");
    assert!(matches!(h.widget.state().mode.get(), Mode::EditNode(_)));
    {
        let doc = h.widget.state().doc.borrow();
        assert!(doc.edges.iter().any(|x| x.source.0 == "a" && x.dest.0 == new_id));
    }
    // The editor textarea is shown and focused; typing updates the node
    let textarea = document().query_selector(".gd_textarea").unwrap().expect("textarea").dyn_into::<HtmlTextAreaElement>().unwrap();
    assert_eq!(document().active_element().map(|a| a.tag_name().to_lowercase()), Some("textarea".into()));
    textarea.set_value("Hello");
    let init = EventInit::new();
    init.set_bubbles(true);
    textarea.dispatch_event(&Event::new_with_event_init_dict("input", &init).unwrap()).unwrap();
    assert_eq!(h.widget.state().doc.borrow().node(&NodeId(new_id.clone())).unwrap().text, "Hello");
    assert_eq!(node_el(&new_id).text_content().unwrap(), "Hello");
    // Deselecting the node closes its editor
    mouse(&node_el(&new_id), 2);
    assert_eq!(h.widget.state().mode.get(), Mode::Layers);
    mouse(&node_el(&new_id), 2);
    key("e", false);
    assert!(matches!(h.widget.state().mode.get(), Mode::EditNode(_)));
    // Escape leaves edit mode; undo reverts the text
    key("Escape", false);
    assert_eq!(h.widget.state().mode.get(), Mode::Layers);
    key("z", false);
    assert_eq!(h.widget.state().doc.borrow().node(&NodeId(new_id.clone())).unwrap().text, "");
    // New sibling from start
    key("s", false);
    let (s2, e2) = sel(&h);
    assert_eq!(s2, Some("a".into()));
    assert!(e2.is_some() && e2 != Some(new_id.clone()));
    // Unlinked node (new island) from the toolbar key
    key("N", false);
    let (s3, e3) = sel(&h);
    let island = s3.expect("island selected as start");
    assert_eq!(e3, None);
    let doc = h.widget.state().doc.borrow();
    assert!(!doc.edges.iter().any(|x| x.source.0 == island || x.dest.0 == island));
}

#[wasm_bindgen_test]
fn overlay_buttons() {
    let h = setup(sample_doc());
    let overlay = document().query_selector(".gd_overlay").unwrap().expect("overlay");
    let sibling = document().query_selector(".gd_overlay_button[title^=\"New sibling\"]").unwrap().expect("sibling button");
    let next = document().query_selector(".gd_overlay_button[title^=\"New node linked\"]").unwrap().expect("next button");
    // Hidden with no selection
    assert!(overlay.class_list().contains("gd_overlay_hidden"));
    // A lone root has no sibling; the next button sits below it
    mouse(&node_el("a"), 0);
    assert!(!overlay.class_list().contains("gd_overlay_hidden"));
    assert!(sibling.class_list().contains("gd_overlay_hidden"));
    assert_eq!(next.get_attribute("data-side").as_deref(), Some("bottom"));
    let a = h.widget.state().placed(&NodeId("a".into())).unwrap().rect;
    let next_el = next.clone().dyn_into::<HtmlElement>().unwrap();
    let left: f64 = next_el.style().get_property_value("left").unwrap().trim_end_matches("px").parse().unwrap();
    assert!((left - (a.x + a.w / 2. + WORLD_MARGIN)).abs() < 0.01, "{} vs {:?}", left, a);
    // With a link selected the sibling button appears to the right of the end
    mouse(&node_el("b"), 2);
    assert!(!sibling.class_list().contains("gd_overlay_hidden"));
    let b = h.widget.state().placed(&NodeId("b".into())).unwrap().rect;
    let sib_el = sibling.clone().dyn_into::<HtmlElement>().unwrap();
    let left: f64 = sib_el.style().get_property_value("left").unwrap().trim_end_matches("px").parse().unwrap();
    assert!((left - (b.x + b.w + WORLD_MARGIN)).abs() < 0.01);
    // Clicking the sibling button creates a node linked from the start
    let before = h.widget.state().doc.borrow().nodes.len();
    mouse(&sibling, 0);
    sibling.dispatch_event(&MouseEvent::new("click").unwrap()).unwrap();
    let (s, e) = sel(&h);
    assert_eq!(s, Some("a".into()));
    let new_id = e.expect("new sibling selected as end");
    assert_ne!(new_id, "b");
    {
        let doc = h.widget.state().doc.borrow();
        assert_eq!(doc.nodes.len(), before + 1);
        assert!(doc.edges.iter().any(|x| x.source.0 == "a" && x.dest.0 == new_id));
    }
    // The next button goes above the end node when the link is reversed
    key("r", false);
    assert_eq!(next.get_attribute("data-side").as_deref(), Some("top"));
    // Clicking the next button creates a node linked to the end (inward)
    next.dispatch_event(&MouseEvent::new("click").unwrap()).unwrap();
    let (s2, e2) = sel(&h);
    assert_eq!(s2, Some(new_id.clone()));
    let newer = e2.expect("new next selected as end");
    let doc = h.widget.state().doc.borrow();
    assert!(doc.edges.iter().any(|x| x.source.0 == newer && x.dest.0 == new_id));
}

#[wasm_bindgen_test]
fn layers_panel() {
    let h = setup(sample_doc());
    let checkbox = document().query_selector(".gd_layer_row .gd_checkbox").unwrap().expect("layer checkbox").dyn_into::<HtmlInputElement>().unwrap();
    assert!(checkbox.checked());
    checkbox.click();
    assert!(!h.widget.state().doc.borrow().layer(&LayerId("L".into())).unwrap().active);
    // Gamma is hidden now
    assert!(document().query_selector(".gd_node_wrap[data-node=\"c\"]").unwrap().is_none());
    assert_eq!(h.widget.state().layout.borrow().nodes.len(), 4);
    // Select the layer as current
    let name = document().query_selector(".gd_layer_name").unwrap().unwrap().dyn_into::<HtmlElement>().unwrap();
    name.click();
    assert_eq!(h.widget.state().doc.borrow().selected_layer, Some(LayerId("L".into())));
    // Add a layer
    let input = document().query_selector(".gd_layers .gd_input").unwrap().unwrap().dyn_into::<HtmlInputElement>().unwrap();
    input.set_value("New");
    let add = document().query_selector(".gd_layers .gd_text_button").unwrap().unwrap().dyn_into::<HtmlElement>().unwrap();
    add.click();
    assert_eq!(h.widget.state().doc.borrow().layers.len(), 2);
}

#[wasm_bindgen_test]
fn search() {
    let h = setup(sample_doc());
    key("/", false);
    assert!(matches!(h.widget.state().mode.get(), Mode::Search(_)));
    let input = document().query_selector(".gd_search .gd_input").unwrap().expect("search input").dyn_into::<HtmlInputElement>().unwrap();
    input.set_value("gam");
    let init = EventInit::new();
    init.set_bubbles(true);
    input.dispatch_event(&Event::new_with_event_init_dict("input", &init).unwrap()).unwrap();
    assert_eq!(document().query_selector_all(".gd_search_row").unwrap().length(), 1);
    let kinit = KeyboardEventInit::new();
    kinit.set_key("Enter");
    kinit.set_bubbles(true);
    kinit.set_cancelable(true);
    input.dispatch_event(&KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &kinit).unwrap()).unwrap();
    assert_eq!(sel(&h), (Some("c".into()), None));
    assert_eq!(h.widget.state().mode.get(), Mode::Layers);
}

#[wasm_bindgen_test]
fn node_moving_into_parent_keeps_element() {
    // Group g is in layer L (hidden), d is inside it. Enabling L moves d into g.
    let mut doc = sample_doc();
    doc.layers[0].active = false;
    doc.nodes.iter_mut().find(|n| n.id.0 == "g").unwrap().layers = vec![LayerId("L".into())];
    let h = setup(doc);
    assert!(document().query_selector(".gd_node_wrap[data-node=\"g\"]").unwrap().is_none());
    let d_before = node_el("d");
    let checkbox = document().query_selector(".gd_layer_row .gd_checkbox").unwrap().unwrap().dyn_into::<HtmlInputElement>().unwrap();
    checkbox.click();
    let layout = h.widget.state().layout.borrow().clone();
    assert!(layout.primary(&NodeId("d".into())).unwrap().id.container == Some(NodeId("g".into())));
    // Same element, now above its container
    assert!(d_before.is_same_node(Some(&node_el("d"))));
    let z = |id: &str| node_el(id).dyn_into::<HtmlElement>().unwrap().style().get_property_value("z-index").unwrap();
    assert_eq!(z("g"), "0");
    assert_eq!(z("d"), "1");
}

#[wasm_bindgen_test]
fn parallel_links_and_edge_editor() {
    let mut doc = sample_doc();
    doc.edges.push(edge("e4", "a", "b"));
    let h = setup(doc);
    mouse(&node_el("a"), 0);
    // Nothing to cycle without a selected link
    key("ArrowRight", false);
    assert_eq!(sel(&h), (Some("a".into()), None));
    // Forward selects a link; then the arrows across the flow cycle a's links
    key("ArrowDown", false);
    let first = h.widget.state().sel_edge.get().unwrap();
    key("ArrowRight", false);
    let second = h.widget.state().sel_edge.get().unwrap();
    assert_ne!(first, second);
    key("ArrowLeft", false);
    assert_eq!(h.widget.state().sel_edge.get().unwrap(), first);
    // Both links to b are visited separately
    let mut seen = vec![first.clone(), second.clone()];
    key("ArrowRight", false);
    key("ArrowRight", false);
    seen.push(h.widget.state().sel_edge.get().unwrap());
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3);
    // Edge editor closes when the link is deselected
    key("L", false);
    assert!(matches!(h.widget.state().mode.get(), Mode::EditEdge(_)));
    key("Escape", false);
    key("Escape", false);
    assert_eq!(sel(&h), (Some("a".into()), None));
    assert_eq!(h.widget.state().mode.get(), Mode::Layers);
    key("ArrowDown", false);
    key("L", false);
    let edited = h.widget.state().sel_edge.get().unwrap();
    assert_eq!(h.widget.state().mode.get(), Mode::EditEdge(edited));
    key("Escape", false);
    mouse(&node_el("a"), 0);
    assert_eq!(h.widget.state().mode.get(), Mode::Layers);
    // Zoom is shown
    assert_eq!(document().query_selector(".gd_zoom").unwrap().unwrap().text_content().unwrap(), "100%");
}

/// Screen position of a world point.
fn screen_point(h: &Harness, p: grafdag_core::layout::Pt) -> (f64, f64) {
    let canvas = document().query_selector(".gd_canvas").unwrap().unwrap();
    let r = canvas.get_bounding_client_rect();
    let (px, py) = h.widget.state().pan.get();
    let z = h.widget.state().zoom.get();
    let m = grafdag_web::widget::render::WORLD_MARGIN;
    return (r.left() + px + (p.x + m) * z, r.top() + py + (p.y + m) * z);
}

#[wasm_bindgen_test]
fn links_are_hit_testable() {
    let h = setup(sample_doc());
    let layout = h.widget.state().layout.borrow().clone();
    let e2 = layout.edges.iter().find(|e| e.id.0 == "e2").unwrap();
    // Midpoint of the first segment
    let a = e2.points[0];
    let b = e2.points[1];
    let mid = grafdag_core::layout::Pt {
        x: (a.x + b.x) / 2.,
        y: (a.y + b.y) / 2.,
    };
    let (sx, sy) = screen_point(&h, mid);
    let target = document().element_from_point(sx as f32, sy as f32).expect("something under the point");
    assert_eq!(target.get_attribute("class").unwrap_or_default(), "gd_edge_hit", "got {} {:?}", target.tag_name(), target.get_attribute("class"));
    assert_eq!(target.get_attribute("data-edge"), Some("e2".into()));
    // Slightly off the line still hits (wide hit path)
    let target = document().element_from_point(sx as f32 + 4., sy as f32 + 4.).unwrap();
    assert_eq!(target.get_attribute("data-edge"), Some("e2".into()), "got {} {:?}", target.tag_name(), target.get_attribute("class"));
    // Zoomed out, the hit width stays the same on screen
    h.widget.state().eg.event(|pc| h.widget.state().cmd_zoom(pc, 0.25, None));
    let (sx, sy) = screen_point(&h, mid);
    let target = document().element_from_point(sx as f32 + 4., sy as f32 + 4.).unwrap();
    assert_eq!(target.get_attribute("data-edge"), Some("e2".into()));
}

#[wasm_bindgen_test]
async fn layer_fade_animates_selection_does_not() {
    let mut doc = sample_doc();
    doc.layers.push(Layer {
        id: LayerId("M".into()),
        name: "Layer M".into(),
        active: true,
    });
    doc.nodes.iter_mut().find(|n| n.id.0 == "d").unwrap().layers = vec![LayerId("M".into())];
    // A layer-colored link still turns the selection color when selected
    doc.edges.iter_mut().find(|e| e.id.0 == "e1").unwrap().layer = Some(LayerId("L".into()));
    let h = setup(doc);
    h.widget.state().animate.set(true);
    let opacity = |id: &str| node_el(id).dyn_into::<HtmlElement>().unwrap().style().get_property_value("opacity").unwrap().parse::<f64>().unwrap();
    assert_eq!(opacity("a"), 0.6);
    // Selection is immediate
    mouse(&node_el("a"), 0);
    mouse(&node_el("b"), 2);
    assert_eq!(opacity("a"), 1.);
    assert_eq!(opacity("b"), 1.);
    let path = document().query_selector(".gd_edge[data-edge=\"e1\"]").unwrap().unwrap();
    assert!(path.class_list().contains("gd_edge_selected"));
    assert_eq!(path.clone().dyn_into::<web_sys::SvgElement>().unwrap().style().get_property_value("opacity").unwrap().parse::<f64>().unwrap(), 1.);
    let stroke = gloo_utils::window().get_computed_style(&path).unwrap().unwrap().get_property_value("stroke").unwrap();
    assert_eq!(stroke, "rgb(0, 0, 0)");
    // Hover is immediate
    let hover_init = MouseEventInit::new();
    node_el("c").dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseenter", &hover_init).unwrap()).unwrap();
    assert_eq!(opacity("c"), 1.);
    assert_eq!(opacity("a"), 1.);
    node_el("c").dispatch_event(&MouseEvent::new_with_mouse_event_init_dict("mouseleave", &hover_init).unwrap()).unwrap();
    assert_eq!(opacity("c"), 0.6);
    assert_eq!(opacity("a"), 1.);
    // Making layer L current fades things outside it, eased
    let name = document().query_selector(".gd_layer_name").unwrap().unwrap().dyn_into::<HtmlElement>().unwrap();
    name.click();
    assert!(opacity("d") > 0.3 && opacity("d") <= 0.6, "{}", opacity("d"));
    gloo_timers::future::TimeoutFuture::new(600).await;
    assert_eq!(opacity("d"), 0.3);
    assert_eq!(opacity("c"), 0.6);
    assert_eq!(opacity("a"), 1.);
}

/// With the flow rotated to the right, successors lie to the right, the
/// arrow keys follow the flow and the overlay buttons move to the matching
/// sides.
#[wasm_bindgen_test]
fn rotated_flow() {
    let h = setup(sample_doc());
    let rotate = document().query_selector(".gd_button[title^=\"Rotate layout\"]").unwrap().expect("rotate button");
    rotate.dispatch_event(&MouseEvent::new("click").unwrap()).unwrap();
    assert_eq!(h.widget.state().doc.borrow().flow, grafdag_core::layout::Flow::Right);
    let layout = h.widget.state().layout.borrow().clone();
    assert_eq!(layout.flow, grafdag_core::layout::Flow::Right);
    let a = layout.primary(&NodeId("a".into())).unwrap().rect;
    let b = layout.primary(&NodeId("b".into())).unwrap().rect;
    let c = layout.primary(&NodeId("c".into())).unwrap().rect;
    assert!(b.x >= a.right() && c.x >= a.right(), "successors to the right: {:?} {:?} {:?}", a, b, c);
    assert!((b.cy() - c.cy()).abs() > 10., "siblings spread vertically");
    // Edges leave a on its right side
    let e1 = layout.edges.iter().find(|e| e.id.0 == "e1").unwrap();
    assert!((e1.points[0].x - a.right()).abs() < 0.01, "{:?} vs {:?}", e1.points[0], a);
    // Right is now forward
    key("PageDown", false);
    assert_eq!(sel(&h), (Some("a".into()), None));
    key("ArrowRight", false);
    let (s, e) = sel(&h);
    assert_eq!(s, Some("a".into()));
    assert!(e == Some("b".into()) || e == Some("c".into()));
    // Down cycles siblings (links on the same side of a)
    key("ArrowDown", false);
    let (_, e2) = sel(&h);
    assert!(e2.is_some() && e2 != e);
    key("ArrowUp", false);
    assert_eq!(sel(&h).1, e);
    // Left is backward: it swaps start and end since the selection points forward
    key("ArrowLeft", false);
    assert_eq!(sel(&h), (e.clone(), Some("a".into())));
    // The overlay buttons: sibling below the end node, next to its left (the
    // selected link now points backward, so a new link would too)
    let sibling = document().query_selector(".gd_overlay_button[title^=\"New sibling\"]").unwrap().expect("sibling button");
    let next = document().query_selector(".gd_overlay_button[title^=\"New node linked\"]").unwrap().expect("next button");
    assert_eq!(sibling.get_attribute("data-side").as_deref(), Some("bottom"));
    assert_eq!(next.get_attribute("data-side").as_deref(), Some("left"));
    // Undo restores the flow
    key("z", false);
    assert_eq!(h.widget.state().doc.borrow().flow, grafdag_core::layout::Flow::Down);
    assert_eq!(h.widget.state().layout.borrow().flow, grafdag_core::layout::Flow::Down);
}
