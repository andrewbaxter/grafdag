//! Canvas rendering: nodes (divs with a selection wrapper), edges (svg paths),
//! labels. Node text is measured in the DOM to size nodes. Positions and
//! opacities are animated prims so re-layouts and fade changes transition.
use {
    super::{
        anim::{
            ease,
            TRANSITION_MS,
        },
        state::{
            Measured,
            Overlay,
            State,
        },
    },
    gloo_events::EventListenerOptions,
    gloo_utils::document,
    grafdag_core::{
        layout::{
            Layout,
            LayoutConfig,
            Motion,
            ScreenDir,
            NodeSize,
            PlacementId,
            Pt,
            Rect,
        },
        EdgeId,
        NodeId,
    },
    lunk::{
        link,
        HistPrim,
        HistPrimEaseExt,
        Link,
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
        ops::{
            Add,
            Mul,
            Sub,
        },
        rc::{
            Rc,
            Weak,
        },
    },
    wasm_bindgen::JsCast,
    web_sys::HtmlElement,
};

const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// Margin around the layout in world coordinates.
pub const WORLD_MARGIN: f64 = 40.;

// Animatable values

/// A rectangle as an animatable value.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect4(pub [f64; 4]);

impl From<Rect> for Rect4 {
    fn from(r: Rect) -> Self {
        return Rect4([r.x, r.y, r.w, r.h]);
    }
}

impl Add for Rect4 {
    type Output = Rect4;

    fn add(self, o: Rect4) -> Rect4 {
        return Rect4([self.0[0] + o.0[0], self.0[1] + o.0[1], self.0[2] + o.0[2], self.0[3] + o.0[3]]);
    }
}

impl Sub for Rect4 {
    type Output = Rect4;

    fn sub(self, o: Rect4) -> Rect4 {
        return Rect4([self.0[0] - o.0[0], self.0[1] - o.0[1], self.0[2] - o.0[2], self.0[3] - o.0[3]]);
    }
}

impl Mul<f64> for Rect4 {
    type Output = Rect4;

    fn mul(self, k: f64) -> Rect4 {
        return Rect4([self.0[0] * k, self.0[1] * k, self.0[2] * k, self.0[3] * k]);
    }
}

/// An edge's polyline and label position as an animatable value. Arithmetic
/// is element-wise and requires equal point counts (otherwise the value is
/// snapped rather than eased).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EdgeGeom {
    pub points: Vec<Pt>,
    pub label: Pt,
}

impl Add for EdgeGeom {
    type Output = EdgeGeom;

    fn add(self, o: EdgeGeom) -> EdgeGeom {
        return EdgeGeom {
            points: self.points.iter().zip(o.points.iter()).map(|(a, b)| Pt {
                x: a.x + b.x,
                y: a.y + b.y,
            }).collect(),
            label: Pt {
                x: self.label.x + o.label.x,
                y: self.label.y + o.label.y,
            },
        };
    }
}

impl Sub for EdgeGeom {
    type Output = EdgeGeom;

    fn sub(self, o: EdgeGeom) -> EdgeGeom {
        return EdgeGeom {
            points: self.points.iter().zip(o.points.iter()).map(|(a, b)| Pt {
                x: a.x - b.x,
                y: a.y - b.y,
            }).collect(),
            label: Pt {
                x: self.label.x - o.label.x,
                y: self.label.y - o.label.y,
            },
        };
    }
}

impl Mul<f64> for EdgeGeom {
    type Output = EdgeGeom;

    fn mul(self, k: f64) -> EdgeGeom {
        return EdgeGeom {
            points: self.points.iter().map(|a| Pt {
                x: a.x * k,
                y: a.y * k,
            }).collect(),
            label: Pt {
                x: self.label.x * k,
                y: self.label.y * k,
            },
        };
    }
}

/// A drawn node: the wrapper element plus its animated rect and opacity.
pub struct NodeView {
    pub el: El,
    pub rect: HistPrim<Rect4>,
    pub opacity: HistPrim<f64>,
    /// Outside the current layer (faded more).
    pub secondary: bool,
    /// Not yet drawn: appears in place rather than easing in.
    pub fresh: bool,
    _links: Vec<Link>,
}

pub type EdgeKey = (EdgeId, PlacementId, PlacementId);

/// A drawn edge: path (and label) elements plus animated geometry and opacity.
pub struct EdgeView {
    pub el: El,
    /// Invisible wide path for clicking.
    pub hit: El,
    pub label: Option<El>,
    pub geom: HistPrim<EdgeGeom>,
    pub opacity: HistPrim<f64>,
    pub source: NodeId,
    pub dest: NodeId,
    pub secondary: bool,
    pub fresh: bool,
    _links: Vec<Link>,
}

// DOM helpers

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

fn write_rect(e: &El, r: &Rect4) {
    set_style(e, "left", &px(r.0[0] + WORLD_MARGIN));
    set_style(e, "top", &px(r.0[1] + WORLD_MARGIN));
    set_style(e, "width", &px(r.0[2]));
    set_style(e, "height", &px(r.0[3]));
}

fn write_opacity(e: &El, v: f64) {
    set_style(e, "opacity", &format!("{:.3}", v));
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

fn write_edge_geom(path: &El, hit: &El, label: Option<&El>, g: &EdgeGeom) {
    let d = path_d(&g.points);
    path.ref_attr("d", &d);
    hit.ref_attr("d", &d);
    if let Some(l) = label {
        set_style(l, "left", &px(g.label.x + WORLD_MARGIN));
        set_style(l, "top", &px(g.label.y + WORLD_MARGIN));
    }
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

// Measurement

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

// Node elements

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
            ev.stop_propagation();
            ev.prevent_default();
            state.eg.event(|pc| {
                state.activate_node_layer(pc, &id);
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

fn make_node_view(pc: &mut ProcessingContext, state: &Rc<State>, id: &PlacementId) -> NodeView {
    let e = make_node_el(&Rc::downgrade(state), id);
    let rect = HistPrim::new(pc, Rect4::default());
    let opacity = HistPrim::new(pc, state.fade.get());
    // These links apply animation steps (the immediate paths write the DOM
    // directly)
    let links = vec![link!((_pc = pc), (rect = rect.clone()), (), (e = e.weak(), state = Rc::downgrade(state), id = id.clone()) {
        write_rect(&e.upgrade()?, &rect.get());
        // Overlay buttons follow the focused node's primary placement
        let state = state.upgrade()?;
        if state.focus_node().as_ref() == Some(&id.node) && state.layout.borrow().primary(&id.node).map(|p| &p.id == id).unwrap_or(false) {
            place_overlay(&state, Some(&rect.get()));
        }
    }), link!((_pc = pc), (opacity = opacity.clone()), (), (e = e.weak()) {
        write_opacity(&e.upgrade()?, opacity.get());
    })];
    return NodeView {
        el: e,
        rect: rect,
        opacity: opacity,
        secondary: false,
        fresh: true,
        _links: links,
    };
}

/// Create node elements for visible nodes, remove stale ones, measure text.
/// Returns the sizes for the layout.
fn sync_nodes(pc: &mut ProcessingContext, state: &Rc<State>, nodes_el: &El) -> HashMap<NodeId, NodeSize> {
    let doc = state.doc.borrow();
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
    let stale: Vec<PlacementId> = render.nodes.keys().filter(|k| !wanted_set.contains(k)).cloned().collect();
    // A node whose placement changed (e.g. moved into a parent that became
    // visible) keeps its element so it animates to its new place
    for id in &wanted {
        if render.nodes.contains_key(id) {
            continue;
        }
        if let Some(old) = stale.iter().find(|k| k.node == id.node && !render.nodes.contains_key(id)) {
            if let Some(v) = render.nodes.remove(old) {
                render.nodes.insert(id.clone(), v);
            }
        }
    }
    for k in stale {
        if let Some(v) = render.nodes.remove(&k) {
            v.el.ref_replace(vec![]);
        }
    }
    let mut sizes = HashMap::new();
    for id in &wanted {
        let node = doc.node(&id.node).unwrap();
        let is_new = !render.nodes.contains_key(id);
        if is_new {
            let v = make_node_view(pc, state, id);
            nodes_el.ref_push(v.el.clone());
            render.nodes.insert(id.clone(), v);
        }
        let e = &render.nodes[id].el;
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
        let e = &render.nodes[id].el;
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

// Applying layout and selection

/// Set an animated prim, easing if enabled and `animate` is set. The
/// immediate path writes the DOM through `write` since sets inside a link
/// don't reach the prim's own link.
fn set_animated<T>(
    pc: &mut ProcessingContext,
    state: &State,
    prim: &HistPrim<T>,
    value: T,
    animate: bool,
    write: impl FnOnce(&T),
)
where
    T: PartialEq + Clone + Add<T, Output = T> + Sub<T, Output = T> + Mul<f64, Output = T> + 'static {
    if prim.get() == value {
        return;
    }
    if animate && state.animate.get() {
        prim.set_ease(&state.animator, value, TRANSITION_MS, ease);
    } else {
        state.animator.cancel(prim);
        write(&value);
        prim.set(pc, value);
    }
}

fn apply_layout(pc: &mut ProcessingContext, state: &Rc<State>, layout: &Layout, nodes_el: &El, svg_el_: &El, paths_el: &El, labels_el: &El) {
    let doc = state.doc.borrow();
    let mut render = state.render.borrow_mut();

    // Nodes stay flat; children are drawn over their containers by z-index
    let _ = nodes_el;
    for n in &layout.nodes {
        let Some(v) = render.nodes.get_mut(&n.id) else {
            continue;
        };
        let e = v.el.clone();
        set_style(&e, "z-index", &n.depth.to_string());
        // New nodes appear in place; existing ones ease to their new place
        let fresh = v.fresh;
        set_animated(pc, state, &v.rect, Rect4::from(n.rect), !fresh, |r| write_rect(&e, r));
        let node = doc.node(&n.id.node);
        v.secondary = is_secondary(&doc, node.map(|nn| nn.layers.as_slice()).unwrap_or(&[]));
        e.ref_modify_classes(&[("gd_node_container", n.container)]);
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

    // Edges: keep elements across layouts so they can ease
    let mut wanted: HashSet<EdgeKey> = HashSet::new();
    for e in &layout.edges {
        if e.points.len() < 2 {
            continue;
        }
        let key: EdgeKey = (e.id.clone(), e.source.clone(), e.dest.clone());
        wanted.insert(key.clone());
        if !render.edges.contains_key(&key) {
            // Carry over the element when only the endpoint placements changed
            let old = render.edges.keys().find(|k| k.0 == key.0 && k.1.node == key.1.node && k.2.node == key.2.node && !wanted.contains(*k)).cloned();
            if let Some(old) = old {
                if let Some(v) = render.edges.remove(&old) {
                    render.edges.insert(key.clone(), v);
                }
            }
        }
        let edge = doc.edge(&e.id);
        let text = edge.map(|x| x.text.clone()).unwrap_or_default();
        let geom = EdgeGeom {
            points: e.points.clone(),
            label: e.label,
        };
        if !render.edges.contains_key(&key) {
            let p = svg_el("path").classes(&["gd_edge"]);
            p.ref_attr("data-edge", &e.id.0);
            let hit = svg_el("path").classes(&["gd_edge_hit"]);
            hit.ref_attr("data-edge", &e.id.0);
            // Clicking a link selects its ends (start = source, end = dest)
            hit.ref_on_with_options("mousedown", EventListenerOptions::enable_prevent_default(), {
                let state = Rc::downgrade(state);
                let id = e.id.clone();
                move |ev| {
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    ev.stop_propagation();
                    ev.prevent_default();
                    state.eg.event(|pc| {
                        state.set_edge(pc, &id);
                    });
                }
            });
            hit.ref_on_with_options("contextmenu", EventListenerOptions::enable_prevent_default(), |ev| {
                ev.prevent_default();
                ev.stop_propagation();
            });
            for (event, entering) in [("mouseenter", true), ("mouseleave", false)] {
                hit.ref_on(event, {
                    let state = Rc::downgrade(state);
                    let id = e.id.clone();
                    move |_| {
                        let Some(state) = state.upgrade() else {
                            return;
                        };
                        state.eg.event(|pc| {
                            if entering {
                                state.hover_edge.set(pc, Some(id.clone()));
                            } else if state.hover_edge.get().as_ref() == Some(&id) {
                                state.hover_edge.set(pc, None);
                            }
                        });
                    }
                });
            }
            paths_el.ref_push(p.clone());
            paths_el.ref_push(hit.clone());
            let geom_prim = HistPrim::new(pc, geom.clone());
            let opacity = HistPrim::new(pc, state.fade.get());
            render.edges.insert(key.clone(), EdgeView {
                el: p,
                hit: hit,
                label: None,
                geom: geom_prim,
                opacity: opacity,
                source: e.source.node.clone(),
                dest: e.dest.node.clone(),
                secondary: false,
                fresh: true,
                _links: vec![],
            });
        }
        let v = render.edges.get_mut(&key).unwrap();
        // Label element: present iff the edge has text
        match (&v.label, text.is_empty()) {
            (Some(l), true) => {
                l.ref_replace(vec![]);
                v.label = None;
            },
            (Some(l), false) => {
                l.ref_text(&text);
            },
            (None, false) => {
                let l = el("div").classes(&["gd_edge_label"]).text(&text);
                labels_el.ref_push(l.clone());
                v.label = Some(l);
            },
            (None, true) => { },
        }
        // (Re)create the links so they see the current label element
        let path_weak = v.el.weak();
        let hit_weak = v.hit.weak();
        let label_weak = v.label.as_ref().map(|l| l.weak());
        v._links = vec![link!((_pc = pc), (geom = v.geom.clone()), (), (path = path_weak.clone(), hit = hit_weak, label = label_weak.clone()) {
            let label = label.as_ref().and_then(|l| l.upgrade());
            write_edge_geom(&path.upgrade()?, &hit.upgrade()?, label.as_ref(), &geom.get());
        }), link!((_pc = pc), (opacity = v.opacity.clone()), (), (path = path_weak, label = label_weak) {
            write_opacity(&path.upgrade()?, opacity.get());
            if let Some(l) = label.as_ref().and_then(|l| l.upgrade()) {
                write_opacity(&l, opacity.get());
            }
        })];
        v.secondary = is_secondary(&doc, edge.and_then(|x| x.layer.clone()).as_slice());
        v.el.ref_modify_classes(&[("gd_edge_reversed", e.reversed)]);
        match edge.and_then(|edge| edge.layer.as_ref()).and_then(|l| layer_color_index(&doc, l)) {
            Some(i) => v.el.ref_attr("data-layer-color", &i.to_string()),
            None => v.el.ref_remove_attr("data-layer-color"),
        };
        let same_shape = v.geom.get().points.len() == geom.points.len();
        let path = v.el.clone();
        let hit = v.hit.clone();
        let label = v.label.clone();
        let fresh = v.fresh;
        set_animated(pc, state, &v.geom, geom, same_shape && !fresh, |g| write_edge_geom(&path, &hit, label.as_ref(), g));
    }
    let stale: Vec<EdgeKey> = render.edges.keys().filter(|k| !wanted.contains(k)).cloned().collect();
    for k in stale {
        if let Some(v) = render.edges.remove(&k) {
            v.el.ref_replace(vec![]);
            v.hit.ref_replace(vec![]);
            if let Some(l) = v.label {
                l.ref_replace(vec![]);
            }
        }
    }
    drop(render);
    drop(doc);
    apply_selection(pc, state, true);
}

/// Selection borders and fading. Selected nodes and their edges are unfaded;
/// a hovered node (and its edges) or a hovered link (and its ends) are unfaded
/// too, in addition to the selection. `animate` eases opacity changes (used
/// for layer and fade level changes, not for selection or hover).
fn apply_selection(pc: &mut ProcessingContext, state: &Rc<State>, animate: bool) {
    let mut render = state.render.borrow_mut();
    let start = state.sel_start.get();
    let end = state.sel_end.get();
    let sel_edge = state.sel_edge.get();
    let hover = state.hover.get();
    let hover_edge = state.hover_edge.get();
    // Nodes whose incident edges are also active: the focused end of the
    // selection (the end node if there is one, else the start node) and a
    // hovered node.
    let spreading: Vec<NodeId> = end.as_ref().or(start.as_ref()).into_iter().chain(hover.iter()).cloned().collect();
    // All active nodes: the spreading set plus every selected node and the ends
    // of a hovered edge. Those are unfaded themselves, but their other edges are
    // not (highlighting only ever reaches immediate neighbors).
    let mut active = spreading.clone();
    active.extend(start.iter().chain(end.iter()).cloned());
    if let Some(h) = &hover_edge {
        for (key, v) in render.edges.iter() {
            if &key.0 == h {
                active.push(v.source.clone());
                active.push(v.dest.clone());
            }
        }
    }
    let fade = state.fade.get();
    let fade_secondary = state.fade_secondary.get();
    let target = |is_active: bool, secondary: bool| {
        if is_active {
            1.
        } else if secondary {
            fade_secondary
        } else {
            fade
        }
    };
    for (id, v) in render.nodes.iter_mut() {
        let is_active = active.contains(&id.node);
        v.el.ref_modify_classes(
            &[
                ("gd_node_start", Some(&id.node) == start.as_ref()),
                ("gd_node_end", Some(&id.node) == end.as_ref()),
                ("gd_active", is_active),
            ],
        );
        let e = v.el.clone();
        set_animated(pc, state, &v.opacity, target(is_active, v.secondary), animate && !v.fresh, |o| write_opacity(&e, *o));
        v.fresh = false;
    }
    // Overlay buttons next to the focused node
    let focus_rect = state.focus_node().and_then(|f| {
        let p = state.layout.borrow().primary(&f)?.id.clone();
        Some(render.nodes.get(&p)?.rect.get())
    });
    place_overlay(state, focus_rect.as_ref());
    for (key, v) in render.edges.iter_mut() {
        let selected = sel_edge.as_ref() == Some(&key.0);
        let is_active =
            spreading.contains(&v.source) || spreading.contains(&v.dest) || hover_edge.as_ref() == Some(&key.0);
        v.el.ref_modify_classes(&[("gd_edge_selected", selected), ("gd_active", is_active)]);
        let path = v.el.clone();
        let label = v.label.clone();
        set_animated(pc, state, &v.opacity, target(is_active, v.secondary), animate && !v.fresh, |o| {
            write_opacity(&path, *o);
            if let Some(l) = &label {
                write_opacity(l, *o);
            }
        });
        v.fresh = false;
    }
}

// Overlay

fn make_overlay_button(state: &Rc<State>, icon: &str, title: &str, cb: impl Fn(&State, &mut ProcessingContext) + 'static) -> El {
    let b = super::toolbar::icon_button(state, icon, title, cb);
    b.ref_classes(&["gd_overlay_button"]);
    // Don't let the canvas treat this as a click on empty space
    b.ref_on_with_options("mousedown", EventListenerOptions::enable_prevent_default(), |ev| {
        ev.stop_propagation();
        ev.prevent_default();
    });
    b.ref_on_with_options("contextmenu", EventListenerOptions::enable_prevent_default(), |ev| {
        ev.prevent_default();
        ev.stop_propagation();
    });
    return b;
}

fn build_overlay(state: &Rc<State>) -> El {
    let sibling = make_overlay_button(state, "add", "New sibling node (s)", |s, pc| s.cmd_new_sibling(pc));
    sibling.ref_attr("data-side", "right");
    let next = make_overlay_button(state, "add", "New node linked from this one (n)", |s, pc| s.cmd_new_next(pc));
    next.ref_attr("data-side", "bottom");
    let overlay = el("div").classes(&["gd_overlay", "gd_overlay_hidden"]).push(sibling.clone()).push(next.clone());
    *state.overlay.borrow_mut() = Some(Overlay {
        el: overlay.clone(),
        sibling: sibling,
        next: next,
    });
    return overlay;
}

/// Position the overlay buttons around the focused node's box (`rect`), or
/// hide them if there's no focused node. The sibling button sits on the
/// node's side-axis side; the next button on its forward side (backward, when
/// the selected link points backwards so a new link would too).
pub fn place_overlay(state: &State, rect: Option<&Rect4>) {
    let overlay = state.overlay.borrow();
    let Some(ov) = overlay.as_ref() else {
        return;
    };
    let Some(r) = rect else {
        ov.el.ref_modify_classes(&[("gd_overlay_hidden", true)]);
        return;
    };
    ov.el.ref_modify_classes(&[("gd_overlay_hidden", false)]);
    let flow = state.doc.borrow().flow;
    ov.sibling.ref_modify_classes(&[("gd_overlay_hidden", !state.sibling_possible())]);
    place_overlay_button(&ov.sibling, r, flow.screen_dir(Motion::SideNext));
    let inward = state.sel_end.get().is_some() && state.inward_direction();
    place_overlay_button(&ov.next, r, flow.screen_dir(if inward {
        Motion::Backward
    } else {
        Motion::Forward
    }));
}

/// Anchor an overlay button to the middle of one side of a box.
fn place_overlay_button(button: &El, r: &Rect4, side: ScreenDir) {
    let [x, y, w, h] = r.0;
    let (name, ax, ay) = match side {
        ScreenDir::Right => ("right", x + w, y + h / 2.),
        ScreenDir::Left => ("left", x, y + h / 2.),
        ScreenDir::Down => ("bottom", x + w / 2., y + h),
        ScreenDir::Up => ("top", x + w / 2., y),
    };
    button.ref_attr("data-side", name);
    set_style(button, "left", &px(ax + WORLD_MARGIN));
    set_style(button, "top", &px(ay + WORLD_MARGIN));
}

pub fn build_canvas(pc: &mut ProcessingContext, state: &Rc<State>) -> El {
    let svg = svg_el("svg").classes(&["gd_svg"]);
    let defs = svg_el("defs");
    for (id, class) in [("gd_arrow", "gd_arrowhead"), ("gd_arrow_sel", "gd_arrowhead_sel")] {
        let marker =
            svg_el("marker")
                .attr("id", id)
                .attr("viewBox", "0 0 10 10")
                .attr("refX", "9")
                .attr("refY", "5")
                .attr("markerWidth", "7")
                .attr("markerHeight", "7")
                .attr("orient", "auto")
                .attr("markerUnits", "userSpaceOnUse")
                .push(svg_el("path").attr("d", "M0,0 L10,5 L0,10 z").classes(&[class]));
        defs.ref_push(marker);
    }
    let paths = svg_el("g").classes(&["gd_paths"]);
    svg.ref_push(defs);
    svg.ref_push(paths.clone());
    let nodes = el("div").classes(&["gd_nodes"]);
    let labels = el("div").classes(&["gd_labels"]);
    let overlay = build_overlay(state);
    let world = el("div").classes(&["gd_world"]).push(svg.clone()).push(labels.clone()).push(nodes.clone()).push(overlay);
    let canvas = el("div").classes(&["gd_canvas"]).push(world.clone());

    // Layout when the document (or the available width) changes
    canvas.ref_own(|_| link!((pc = pc), (version = state.doc_version.clone(), width = state.layout_width.clone(), height = state.layout_height.clone()), (layout = state.layout.clone()), (state = Rc::downgrade(state), nodes = nodes.clone()) {
        let _ = version;
        let state = state.upgrade()?;
        let sizes = sync_nodes(pc, &state, nodes);
        let previous = layout.borrow().clone();
        let new_layout = {
            let doc = state.doc.borrow();
            let mut config = LayoutConfig::default();
            config.flow = doc.flow;
            // Ranks wider than the canvas (along the side axis) are wrapped
            let side_extent = if doc.flow.horizontal() {
                height.get()
            } else {
                width.get()
            };
            config.max_rank_width = Some((side_extent as f64 - 2. * WORLD_MARGIN).max(200.));
            grafdag_core::layout::layout(&doc, &sizes, &config, Some(&previous))
        };
        layout.set(pc, Rc::new(new_layout));
    }));

    // Render when the layout changes
    canvas.ref_own(|_| link!((pc = pc), (layout = state.layout.clone()), (), (state = Rc::downgrade(state), nodes = nodes.clone(), svg = svg.clone(), paths = paths.clone(), labels = labels.clone()) {
        let state = state.upgrade()?;
        apply_layout(pc, &state, &layout.borrow(), nodes, svg, paths, labels);
    }));

    // Following the selection after a re-layout or selection change
    canvas.ref_own(|_| link!((pc = pc), (start = state.sel_start.clone(), end = state.sel_end.clone(), layout = state.layout.clone()), (pan = state.pan.clone()), (state = Rc::downgrade(state)) {
        let _ = (start, end, layout, pan);
        let state = state.upgrade()?;
        state.follow_selection(pc);
    }));

    // Selection and hover: immediate (only layer/fade changes ease)
    canvas.ref_own(|_| link!((pc = pc), (start = state.sel_start.clone(), end = state.sel_end.clone(), sel_edge = state.sel_edge.clone(), hover = state.hover.clone(), hover_edge = state.hover_edge.clone()), (mode = state.mode.clone()), (state = Rc::downgrade(state)) {
        let _ = (start, end, sel_edge, hover, hover_edge);
        let state = state.upgrade()?;
        apply_selection(pc, &state, false);
        // Editors close when their subject is deselected
        match mode.get() {
            super::state::Mode::EditNode(id) => {
                if state.sel_start.get().as_ref() != Some(&id) && state.sel_end.get().as_ref() != Some(&id) {
                    mode.set(pc, super::state::Mode::Layers);
                }
            },
            super::state::Mode::EditEdge(id) => {
                if state.sel_edge.get().as_ref() != Some(&id) {
                    mode.set(pc, super::state::Mode::Layers);
                }
            },
            _ => { },
        }
    }));

    // Fade level changes ease
    canvas.ref_own(|_| link!((pc = pc), (fade = state.fade.clone(), fade_secondary = state.fade_secondary.clone()), (), (state = Rc::downgrade(state)) {
        let _ = (fade, fade_secondary);
        let state = state.upgrade()?;
        apply_selection(pc, &state, true);
    }));

    // View transform
    canvas.ref_own(|_| link!((_pc = pc), (zoom = state.zoom.clone(), pan = state.pan.clone()), (), (world = world.clone()) {
        let (px, py) = pan.get();
        set_style(world, "transform", &format!("translate({}px, {}px) scale({})", px, py, zoom.get()));
        set_style(world, "--gd-zoom", &zoom.get().to_string());
    }));

    canvas.ref_on_resize({
        let state = Rc::downgrade(state);
        move |_, w, h| {
            if let Some(state) = state.upgrade() {
                state.viewport.set((w, h));
                // Round so small resizes don't relayout
                let round = |v: f64| ((v / 100.).floor() * 100.) as u32;
                state.eg.event(|pc| {
                    state.layout_width.set(pc, round(w));
                    state.layout_height.set(pc, round(h));
                });
            }
        }
    });
    super::interact::attach_canvas(&canvas, state);
    return canvas;
}
