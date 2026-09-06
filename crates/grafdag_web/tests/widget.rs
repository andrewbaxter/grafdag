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

fn setup(doc: Document) -> Harness {
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
    return document().query_selector(&format!(".gd_node[data-node=\"{}\"]", id)).unwrap().expect("node element");
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
    }
    let g = layout.primary(&NodeId("g".into())).unwrap();
    let d = layout.primary(&NodeId("d".into())).unwrap();
    assert!(g.container);
    assert!(d.rect.x > g.rect.x && d.rect.y > g.rect.y + g.title_height);
    assert_eq!(layout.edges.len(), 3);
    assert_eq!(document().query_selector_all(".gd_edge").unwrap().length(), 3);
    assert_eq!(node_el("c").get_attribute("data-layer-color"), Some("0".into()));
    // Alpha's text box is one line
    let a = node_el("a").dyn_into::<HtmlElement>().unwrap();
    assert!(a.offset_height() < 40, "{}", a.offset_height());
}

#[wasm_bindgen_test]
fn keyboard_navigation() {
    let h = setup(sample_doc());
    key("Tab", false);
    assert_eq!(sel(&h), (Some("a".into()), None));
    key("ArrowDown", false);
    let (s, e) = sel(&h);
    assert_eq!(s, Some("a".into()));
    assert!(e == Some("b".into()) || e == Some("c".into()));
    // Cycle end among siblings
    key("ArrowRight", false);
    let (_, e2) = sel(&h);
    assert!(e2.is_some() && e2 != e);
    // Shift+Down cycles the end among the start's successors
    key("ArrowDown", true);
    let (_, e3) = sel(&h);
    assert!(e3 == Some("b".into()) || e3 == Some("c".into()));
    // Move forward: start becomes end
    key("ArrowDown", false);
    let (s4, _) = sel(&h);
    assert_eq!(Some(s4.clone().unwrap()), e3);
    // Flip
    key("f", false);
    let (s5, e5) = sel(&h);
    if e5.is_some() {
        assert_eq!(e5, s4);
        assert!(s5.is_some());
    }
    // Escape clears end, then start
    key("Escape", false);
    key("Escape", false);
    assert_eq!(sel(&h), (None, None));
}

#[wasm_bindgen_test]
fn mouse_selection_and_zoom() {
    let h = setup(sample_doc());
    mouse(&node_el("a"), 0);
    assert_eq!(sel(&h), (Some("a".into()), None));
    mouse(&node_el("b"), 2);
    assert_eq!(sel(&h), (Some("a".into()), Some("b".into())));
    assert!(node_el("a").class_list().contains("gd_node_start"));
    assert!(node_el("b").class_list().contains("gd_node_end"));
    assert!(document().query_selector(".gd_edge_between").unwrap().is_some());
    // Toggle off
    mouse(&node_el("b"), 2);
    assert_eq!(sel(&h), (Some("a".into()), None));
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
    assert_eq!(document().query_selector_all(".gd_node").unwrap().length(), 4);
    // Undo restores it, redo deletes again
    key("z", false);
    assert!(h.widget.state().doc.borrow().node(&NodeId("c".into())).is_some());
    assert_eq!(document().query_selector_all(".gd_node").unwrap().length(), 5);
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
}

#[wasm_bindgen_test]
fn layers_panel() {
    let h = setup(sample_doc());
    let checkbox = document().query_selector(".gd_layer_row .gd_checkbox").unwrap().expect("layer checkbox").dyn_into::<HtmlInputElement>().unwrap();
    assert!(checkbox.checked());
    checkbox.click();
    assert!(!h.widget.state().doc.borrow().layer(&LayerId("L".into())).unwrap().active);
    // Gamma is hidden now
    assert!(document().query_selector(".gd_node[data-node=\"c\"]").unwrap().is_none());
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
