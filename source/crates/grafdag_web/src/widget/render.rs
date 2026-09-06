//! Canvas rendering: nodes (divs with a selection wrapper), edges (svg paths),
//! labels. Node text is measured in the DOM to size nodes.
use {
    super::state::{
        Measured,
        State,
    },
    grafdag_core::{
        layout::{
            Layout,
            LayoutConfig,
            NodeSize,
            PlacementId,
            Pt,
        },
        NodeId,
    },
    gloo_utils::document,
    lunk::{
        link,
        ProcessingContext,
    },
    rooting::{
        el,
        el_from_raw,
        El,
    },
    std::{
        collections::{
            HashMap,
            HashSet,
        },
        rc::{
            Rc,
            Weak,
        },
    },
    gloo_events::EventListenerOptions,
    wasm_bindgen::JsCast,
    web_sys::HtmlElement,
};

const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// Margin around the layout in world coordinates.
pub const WORLD_MARGIN: f64 = 40.;

pub fn svg_el(tag: &str) -> El {
    return el_from_raw(document().create_element_ns(Some(SVG_NS), tag).unwrap());
}

pub fn set_style(e: &El, prop: &str, value: &str) {
    if let Some(h) = e.raw().dyn_ref::<HtmlElement>() {
        h.style().set_property(prop, value).unwrap();
    } else if let Some(s) = e.raw().dyn_ref::<web_sys::SvgElement>() {
        s.style().set_property(prop, value).unwrap();
    }
}

pub fn remove_style(e: &El, prop: &str) {
    if let Some(h) = e.raw().dyn_ref::<HtmlElement>() {
        h.style().remove_property(prop).unwrap();
    }
}

fn px(v: f64) -> String {
    return format!("{}px", v);
}

fn set_rect(e: &El, x: f64, y: f64, w: f64, h: f64) {
    set_style(e, "left", &px(x));
    set_style(e, "top", &px(y));
    set_style(e, "width", &px(w));
    set_style(e, "height", &px(h));
}

fn path_d(points: &[Pt]) -> String {
    let mut out = String::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("M{:.1} {:.1}", p.x + WORLD_MARGIN, p.y + WORLD_MARGIN));
        } else {
            out.push_str(&format!(" L{:.1} {:.1}", p.x + WORLD_MARGIN, p.y + WORLD_MARGIN));
        }
    }
    return out;
}

/// The `.gd_node` box inside a node wrapper element.
fn node_inner_el(wrap: &El) -> Option<El> {
    return Some(el_from_raw(wrap.raw().first_element_child()?));
}

/// The `.gd_node_text` element inside a node wrapper element.
fn node_text_el(wrap: &El) -> Option<El> {
    return Some(el_from_raw(wrap.raw().first_element_child()?.first_element_child()?));
}

fn offset_size(e: &El) -> (f64, f64) {
    let h = e.raw().dyn_into::<HtmlElement>().unwrap();
    return (h.offset_width() as f64, h.offset_height() as f64);
}

/// Measure the node's text box, choosing a wrapping width to keep the aspect
/// ratio between 1:2 and 2:1. Downsizing is deferred until the box is 50%
/// mis-sized (hysteresis) so small edits don't reflow everything.
fn measure(wrap_el: &El, previous: Option<&Measured>) -> (NodeSize, Option<f64>) {
    let node_el = &node_inner_el(wrap_el).unwrap();
    let text_el = node_text_el(wrap_el).unwrap();
    remove_style(wrap_el, "width");
    remove_style(wrap_el, "height");
    remove_style(&text_el, "width");
    // The box normally fills the wrapper; shrink it to its content to measure
    set_style(node_el, "width", "max-content");
    let (w0, h0) = offset_size(node_el);
    let (t0, _) = offset_size(&text_el);
    // Horizontal padding + border of the node box
    let chrome = w0 - t0;
    let mut chosen = None;
    // Target a 2:1 aspect ratio, but only start wrapping once the box is 50%
    // wider than that
    if w0 > 3. * h0 && w0 > 0. {
        // Narrowest width without breaking words
        set_style(&text_el, "width", "min-content");
        let (tmin, _) = offset_size(&text_el);
        remove_style(&text_el, "width");
        let mut ideal = ((2. * w0 * h0).sqrt() - chrome).ceil().max(tmin).max(30.);
        if let Some(prev_w) = previous.and_then(|p| p.text_width) {
            if (ideal - prev_w).abs() / prev_w < 0.5 && prev_w >= tmin {
                ideal = prev_w;
            }
        }
        if ideal < t0 {
            chosen = Some(ideal);
        }
    }
    let mut size = NodeSize {
        width: w0,
        height: h0,
    };
    if let Some(mut width) = chosen {
        for _ in 0 .. 4 {
            set_style(&text_el, "width", &px(width));
            let (w1, h1) = offset_size(node_el);
            size = NodeSize {
                width: w1,
                height: h1,
            };
            if h1 > 2. * w1 {
                width *= 1.4;
                chosen = Some(width);
                continue;
            }
            break;
        }
    }
    remove_style(node_el, "width");
    return (size, chosen);
}

fn make_node_el(state: &Weak<State>, id: &PlacementId) -> El {
    let text = el("div").classes(&["gd_node_text"]);
    let inner = el("div").classes(&["gd_node"]).push(text);
    // The wrapper draws the selection border around the node box
    let node = el("div").classes(&["gd_node_wrap"]).push(inner);
    node.ref_on_with_options("mousedown", EventListenerOptions::enable_prevent_default(), {
        let state = state.clone();
        let id = id.node.clone();
        move |ev| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let Some(ev) = ev.dyn_ref::<web_sys::MouseEvent>() else {
                return;
            };
            let button = ev.button();
            if button == 1 {
                return;
            }
            ev.stop_propagation();
            ev.prevent_default();
            state.eg.event(|pc| {
                if button == 0 {
                    if state.sel_start.get().as_ref() == Some(&id) {
                        state.set_start(pc, None);
                    } else {
                        state.set_start(pc, Some(id.clone()));
                        if state.sel_end.get().as_ref() == Some(&id) {
                            state.set_end(pc, None);
                        }
                    }
                } else if button == 2 {
                    if state.sel_end.get().as_ref() == Some(&id) {
                        state.set_end(pc, None);
                    } else if state.sel_start.get().is_none() {
                        state.set_start(pc, Some(id.clone()));
                    } else if state.sel_start.get().as_ref() == Some(&id) {
                        // Right clicking the start node moves it to the end
                        state.set_start(pc, None);
                        state.set_end(pc, Some(id.clone()));
                    } else {
                        state.set_end(pc, Some(id.clone()));
                    }
                }
            });
        }
    });
    node.ref_on("dblclick", {
        let state = state.clone();
        let id = id.node.clone();
        move |ev| {
            let Some(state) = state.upgrade() else {
                return;
            };
            ev.stop_propagation();
            state.eg.event(|pc| {
                state.mode.set(pc, super::state::Mode::EditNode(id.clone()));
            });
        }
    });
    node.ref_on_with_options("contextmenu", EventListenerOptions::enable_prevent_default(), |ev| {
        ev.prevent_default();
        ev.stop_propagation();
    });
    for (event, entering) in [("mouseenter", true), ("mouseleave", false)] {
        node.ref_on(event, {
            let state = state.clone();
            let id = id.node.clone();
            move |_| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                state.eg.event(|pc| {
                    if entering {
                        state.hover.set(pc, Some(id.clone()));
                    } else if state.hover.get().as_ref() == Some(&id) {
                        state.hover.set(pc, None);
                    }
                });
            }
        });
    }
    return node;
}

/// Create node elements for visible nodes, remove stale ones, measure text.
/// Returns the sizes for the layout.
fn sync_nodes(state: &Rc<State>, nodes_el: &El) -> HashMap<NodeId, NodeSize> {
    let doc = state.doc.borrow();
    let weak = Rc::downgrade(state);
    let mut render = state.render.borrow_mut();
    let mut measured = state.measured.borrow_mut();

    // Placements that should exist
    let mut wanted: Vec<PlacementId> = vec![];
    let visible: HashSet<&NodeId> = doc.nodes.iter().filter(|n| doc.node_visible(n)).map(|n| &n.id).collect();
    for n in &doc.nodes {
        if !visible.contains(&n.id) {
            continue;
        }
        let primary = n.parents.iter().find(|p| *p != &n.id && visible.contains(p)).cloned();
        wanted.push(PlacementId {
            node: n.id.clone(),
            container: primary.clone(),
        });
        let mut seen = HashSet::new();
        for p in &n.parents {
            if p == &n.id || !visible.contains(p) || Some(p) == primary.as_ref() || !seen.insert(p.clone()) {
                continue;
            }
            wanted.push(PlacementId {
                node: n.id.clone(),
                container: Some(p.clone()),
            });
        }
    }
    let wanted_set: HashSet<&PlacementId> = wanted.iter().collect();
    let stale: Vec<PlacementId> = render.node_els.keys().filter(|k| !wanted_set.contains(k)).cloned().collect();
    for k in stale {
        if let Some(e) = render.node_els.remove(&k) {
            e.ref_replace(vec![]);
        }
    }
    let mut sizes = HashMap::new();
    for id in &wanted {
        let node = doc.node(&id.node).unwrap();
        let is_new = !render.node_els.contains_key(id);
        if is_new {
            let e = make_node_el(&weak, id);
            nodes_el.ref_push(e.clone());
            render.node_els.insert(id.clone(), e);
        }
        let e = &render.node_els[id];
        let text_changed = measured.get(&id.node).map(|m| m.text != node.text).unwrap_or(true);
        if is_new || text_changed {
            let display = if node.text.is_empty() {
                "\u{00a0}"
            } else {
                node.text.as_str()
            };
            node_text_el(e).unwrap().ref_text(display);
        }
        let ghost = !(node.parents.iter().find(|p| *p != &id.node && visible.contains(p)) == id.container.as_ref());
        if !ghost {
            // Only cache measurements taken while attached to the document
            if text_changed && e.raw().is_connected() {
                let prev = measured.get(&id.node).cloned();
                let (size, width) = measure(e, prev.as_ref());
                measured.insert(id.node.clone(), Measured {
                    text: node.text.clone(),
                    size: size,
                    text_width: width,
                });
            }
            if let Some(m) = measured.get(&id.node) {
                sizes.insert(id.node.clone(), m.size);
            }
        }
        e.ref_modify_classes(&[("gd_node_ghost", ghost)]);
    }
    // Ghosts use the primary's text width
    for id in &wanted {
        let e = &render.node_els[id];
        if let Some(m) = measured.get(&id.node) {
            let text_el = node_text_el(e).unwrap();
            match m.text_width {
                Some(w) => set_style(&text_el, "width", &px(w)),
                None => remove_style(&text_el, "width"),
            }
        }
    }
    return sizes;
}

fn apply_layout(state: &Rc<State>, layout: &Layout, nodes_el: &El, svg_el_: &El, paths_el: &El, labels_el: &El) {
    let doc = state.doc.borrow();
    let mut render = state.render.borrow_mut();

    // Nodes: order by depth so children are drawn over containers
    let mut ordered: Vec<(usize, PlacementId)> = layout.nodes.iter().map(|n| (n.depth, n.id.clone())).collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut els = vec![];
    for (_, id) in &ordered {
        if let Some(e) = render.node_els.get(id) {
            els.push(e.clone());
        }
    }
    nodes_el.ref_clear();
    nodes_el.ref_extend(els);
    for n in &layout.nodes {
        let Some(e) = render.node_els.get(&n.id) else {
            continue;
        };
        set_rect(e, n.rect.x + WORLD_MARGIN, n.rect.y + WORLD_MARGIN, n.rect.w, n.rect.h);
        let node = doc.node(&n.id.node);
        let secondary = is_secondary(&doc, node.map(|nn| nn.layers.as_slice()).unwrap_or(&[]));
        e.ref_modify_classes(&[("gd_node_container", n.container), ("gd_secondary", secondary)]);
        e.ref_attr("data-node", &n.id.node.0);
        // Layers color the outline: the first active layer of the node
        let color = node.and_then(|nn| nn.layers.iter().find(|l| doc.layer_active(l))).and_then(|l| layer_color_index(&doc, l));
        match color {
            Some(i) => e.ref_attr("data-layer-color", &i.to_string()),
            None => e.ref_remove_attr("data-layer-color"),
        };
    }

    // Svg size
    let w = layout.width + 2. * WORLD_MARGIN;
    let h = layout.height + 2. * WORLD_MARGIN;
    svg_el_.ref_attr("width", &format!("{}", w.max(1.)));
    svg_el_.ref_attr("height", &format!("{}", h.max(1.)));
    svg_el_.ref_attr("viewBox", &format!("0 0 {} {}", w.max(1.), h.max(1.)));

    // Outlines and edges
    paths_el.ref_clear();
    labels_el.ref_clear();
    render.edge_els.clear();
    let mut new_paths = vec![];
    let mut new_labels = vec![];
    for e in &layout.edges {
        if e.points.len() < 2 {
            continue;
        }
        let p = svg_el("path").attr("d", &path_d(&e.points)).classes(&["gd_edge"]);
        let edge = doc.edge(&e.id);
        let secondary = is_secondary(&doc, edge.and_then(|x| x.layer.clone()).as_slice());
        p.ref_modify_classes(&[("gd_edge_reversed", e.reversed), ("gd_secondary", secondary)]);
        p.ref_attr("data-edge", &e.id.0);
        if let Some(i) = edge.and_then(|edge| edge.layer.as_ref()).and_then(|l| layer_color_index(&doc, l)) {
            p.ref_attr("data-layer-color", &i.to_string());
        }
        let mut label = None;
        if let Some(edge) = edge {
            if !edge.text.is_empty() {
                let l = el("div").classes(&["gd_edge_label"]).text(&edge.text);
                l.ref_modify_classes(&[("gd_secondary", secondary)]);
                set_style(&l, "left", &px(e.label.x + WORLD_MARGIN));
                set_style(&l, "top", &px(e.label.y + WORLD_MARGIN));
                new_labels.push(l.clone());
                label = Some(l);
            }
        }
        render.edge_els.push((p.clone(), label, e.source.node.clone(), e.dest.node.clone()));
        new_paths.push(p);
    }
    paths_el.ref_extend(new_paths);
    labels_el.ref_extend(new_labels);
}

/// Index of a layer in the document's layer list, used to pick its color
/// (via CSS `data-layer-color`).
fn layer_color_index(doc: &grafdag_core::Document, layer: &grafdag_core::LayerId) -> Option<usize> {
    return doc.layers.iter().position(|l| &l.id == layer).map(|i| i % 8);
}

/// Whether something in these layers belongs to a non-primary layer only
/// (faded more). Things with no layers, or in the selected layer, are primary.
fn is_secondary(doc: &grafdag_core::Document, layers: &[grafdag_core::LayerId]) -> bool {
    let Some(selected) = doc.selected_layer.as_ref().filter(|l| doc.layer_active(l)) else {
        return false;
    };
    if layers.is_empty() {
        return false;
    }
    return !layers.contains(selected);
}

/// Selection borders and fading. Selected nodes and their edges are unfaded;
/// while hovering a node, the hovered node and its edges are the unfaded ones
/// instead.
fn apply_selection(state: &Rc<State>) {
    let render = state.render.borrow();
    let start = state.sel_start.get();
    let end = state.sel_end.get();
    let hover = state.hover.get();
    let active: Vec<NodeId> = match &hover {
        Some(h) => vec![h.clone()],
        None => start.iter().chain(end.iter()).cloned().collect(),
    };
    for (id, e) in &render.node_els {
        e.ref_modify_classes(
            &[
                ("gd_node_start", Some(&id.node) == start.as_ref()),
                ("gd_node_end", Some(&id.node) == end.as_ref()),
                ("gd_active", active.contains(&id.node)),
            ],
        );
    }
    for (e, label, s, d) in &render.edge_els {
        let between = match (&start, &end) {
            (Some(a), Some(b)) => (s == a && d == b) || (s == b && d == a),
            _ => false,
        };
        let is_active = active.contains(s) || active.contains(d);
        e.ref_modify_classes(&[("gd_edge_between", between && hover.is_none()), ("gd_active", is_active)]);
        if let Some(label) = label {
            label.ref_modify_classes(&[("gd_active", is_active)]);
        }
    }
}

pub fn build_canvas(pc: &mut ProcessingContext, state: &Rc<State>) -> El {
    let svg = svg_el("svg").classes(&["gd_svg"]);
    let defs = svg_el("defs");
    let marker =
        svg_el("marker")
            .attr("id", "gd_arrow")
            .attr("viewBox", "0 0 10 10")
            .attr("refX", "9")
            .attr("refY", "5")
            .attr("markerWidth", "7")
            .attr("markerHeight", "7")
            .attr("orient", "auto")
            .attr("markerUnits", "userSpaceOnUse")
            .push(svg_el("path").attr("d", "M0,0 L10,5 L0,10 z").classes(&["gd_arrowhead"]));
    let marker_sel = svg_el("marker")
        .attr("id", "gd_arrow_sel")
        .attr("viewBox", "0 0 10 10")
        .attr("refX", "9")
        .attr("refY", "5")
        .attr("markerWidth", "7")
        .attr("markerHeight", "7")
        .attr("orient", "auto")
        .attr("markerUnits", "userSpaceOnUse")
        .push(svg_el("path").attr("d", "M0,0 L10,5 L0,10 z").classes(&["gd_arrowhead_sel"]));
    defs.ref_push(marker);
    defs.ref_push(marker_sel);
    let paths = svg_el("g").classes(&["gd_paths"]);
    svg.ref_push(defs);
    svg.ref_push(paths.clone());
    let nodes = el("div").classes(&["gd_nodes"]);
    let labels = el("div").classes(&["gd_labels"]);
    let world = el("div").classes(&["gd_world"]).push(svg.clone()).push(labels.clone()).push(nodes.clone());
    let canvas = el("div").classes(&["gd_canvas"]).push(world.clone());

    // Layout when the document changes
    canvas.ref_own(|_| link!((pc = pc), (version = state.doc_version.clone(), width = state.layout_width.clone()), (layout = state.layout.clone()), (state = Rc::downgrade(state), nodes = nodes.clone()) {
        let _ = version;
        let state = state.upgrade()?;
        let sizes = sync_nodes(&state, nodes);
        let previous = layout.borrow().clone();
        let new_layout = {
            let doc = state.doc.borrow();
            let mut config = LayoutConfig::default();
            // Ranks wider than the canvas are wrapped
            config.max_rank_width = Some((width.get() as f64 - 2. * WORLD_MARGIN).max(200.));
            grafdag_core::layout::layout(&doc, &sizes, &config, Some(&previous))
        };
        layout.set(pc, Rc::new(new_layout));
    }));

    // Render when the layout changes
    canvas.ref_own(|_| link!((_pc = pc), (layout = state.layout.clone()), (), (state = Rc::downgrade(state), nodes = nodes.clone(), svg = svg.clone(), paths = paths.clone(), labels = labels.clone()) {
        let state = state.upgrade()?;
        apply_layout(&state, &layout.borrow(), nodes, svg, paths, labels);
        apply_selection(&state);
    }));

    // Selection classes and following
    canvas.ref_own(|_| link!((pc = pc), (start = state.sel_start.clone(), end = state.sel_end.clone(), hover = state.hover.clone(), layout = state.layout.clone()), (pan = state.pan.clone()), (state = Rc::downgrade(state)) {
        let _ = (start, end, hover, layout, pan);
        let state = state.upgrade()?;
        apply_selection(&state);
        state.follow_selection(pc);
    }));

    // View transform
    canvas.ref_own(|_| link!((_pc = pc), (zoom = state.zoom.clone(), pan = state.pan.clone()), (), (world = world.clone()) {
        let (px, py) = pan.get();
        set_style(world, "transform", &format!("translate({}px, {}px) scale({})", px, py, zoom.get()));
    }));

    canvas.ref_on_resize({
        let state = Rc::downgrade(state);
        move |_, w, h| {
            if let Some(state) = state.upgrade() {
                state.viewport.set((w, h));
                // Round so small resizes don't relayout
                let rounded = ((w / 100.).floor() * 100.) as u32;
                state.eg.event(|pc| {
                    state.layout_width.set(pc, rounded);
                });
            }
        }
    });
    super::interact::attach_canvas(&canvas, state);
    return canvas;
}
