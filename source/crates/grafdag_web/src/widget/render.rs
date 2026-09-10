use {
    gloo_events::EventListenerOptions,
    gloo_utils::document,
    grafdag_core::{
        EdgeId,
        NodeId,
        layout::{
            LayoutConfig,
            Motion,
            NodeSize,
            PlacementId,
            Pt,
            Rect,
            ScreenDir,
        },
    },
    lunk::{
        HistPrim,
        HistPrimEaseExt,
        Link,
        ProcessingContext,
        link,
    },
    rooting::{
        El,
        el,
        el_from_raw,
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
        rc::Rc,
    },
    super::{
        anim::{
            TRANSITION_MS,
            ease,
        },
        state::{
            Measured,
            Overlay,
            State,
            Vec2,
        },
    },
    wasm_bindgen::JsCast,
    web_sys::HtmlElement,
};

pub const VIEW_MARGIN: f64 = 40.;

fn apply_selection(pc: &mut ProcessingContext, state: &Rc<State>, animate: bool) {
    let mut render = state.render.borrow_mut();
    let start = state.sel_start.get();
    let end = state.sel_end.get();
    let sel_edge = state.sel_edge.get();
    let hover = state.hover.get();
    let hover_edge = state.hover_edge.get();
    let peek = state.peek.get();
    let (connecting, walk_nodes): (Vec<EdgeId>, Vec<NodeId>) = match (&start, &end) {
        (Some(s), Some(e)) => state.connecting(s, e),
        _ => (vec![], vec![]),
    };
    let mut spreading: Vec<NodeId> = hover.iter().chain(peek.iter()).cloned().collect();
    if start.is_none() {
        spreading.extend(end.iter().cloned());
    }
    let mut active = spreading.clone();
    active.extend(start.iter().chain(end.iter()).cloned());
    active.extend(walk_nodes);
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
        v
            .el
            .ref_modify_classes(
                &[
                    ("gd_node_start", Some(&id.node) == start.as_ref()),
                    ("gd_node_end", Some(&id.node) == end.as_ref()),
                    ("gd_active", is_active),
                ],
            );
        let e = v.el.clone();
        set_animated(
            pc,
            state,
            &v.opacity,
            target(is_active, v.secondary),
            animate && !v.fresh,
            |o| write_opacity(&e, *o),
        );
        v.fresh = false;
    }
    let focus_rect = state.focus_node().and_then(|f| {
        let p = state.layout.borrow().primary(&f)?.id.clone();
        Some(render.nodes.get(&p)?.rect.get())
    });
    place_overlay(state, focus_rect.as_ref());
    for (key, v) in render.edges.iter_mut() {
        let selected = sel_edge.as_ref() == Some(&key.0);
        let is_active =
            spreading.contains(&v.source) || spreading.contains(&v.dest) || connecting.contains(&key.0) ||
                hover_edge.as_ref() == Some(&key.0);
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
    let overlay = {
        let sibling = make_overlay_button(state, "add", "New sibling node (s)", |s, pc| s.cmd_new_sibling(pc));
        sibling.ref_attr("data-side", "right");
        let next =
            make_overlay_button(state, "add", "New node linked from this one (n)", |s, pc| s.cmd_new_next(pc));
        next.ref_attr("data-side", "bottom");
        let overlay =
            el("div").classes(&["gd_overlay", "gd_overlay_hidden"]).push(sibling.clone()).push(next.clone());
        *state.overlay.borrow_mut() = Some(Overlay {
            el: overlay.clone(),
            sibling: sibling,
            next: next,
        });
        overlay
    };
    let world =
        el("div")
            .classes(&["gd_world"])
            .push(svg.clone())
            .push(labels.clone())
            .push(nodes.clone())
            .push(overlay);
    let canvas = el("div").classes(&["gd_canvas"]).push(world.clone());
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (
                version = state.doc_version.clone(),
                width = state.layout_width.clone(),
                height = state.layout_height.clone(),
            ),
            (layout = state.layout.clone()),
            (state = Rc::downgrade(state), nodes = nodes.clone()) {
                let _ = version;
                let state = state.upgrade()?;
                let sizes = {
                    let state = &state;
                    let nodes_el = nodes;
                    let doc = state.doc.borrow();
                    let mut render = state.render.borrow_mut();
                    let mut measured = state.measured.borrow_mut();
                    let mut wanted: Vec<PlacementId> = vec![];
                    let visible: HashSet<&NodeId> =
                        doc.nodes.iter().filter(|n| doc.node_visible(n)).map(|n| &n.id).collect();
                    for n in &doc.nodes {
                        if !visible.contains(&n.id) {
                            continue;
                        }
                        let parents = doc.visible_parents(&n.id);
                        let primary = parents.first().cloned();
                        wanted.push(PlacementId {
                            node: n.id.clone(),
                            container: primary.clone(),
                        });
                        for p in &parents {
                            if Some(p) == primary.as_ref() {
                                continue;
                            }
                            wanted.push(PlacementId {
                                node: n.id.clone(),
                                container: Some(p.clone()),
                            });
                        }
                    }
                    let wanted_set: HashSet<&PlacementId> = wanted.iter().collect();
                    let stale: Vec<PlacementId> =
                        render.nodes.keys().filter(|k| !wanted_set.contains(k)).cloned().collect();
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
                    let containers: HashSet<NodeId> = wanted.iter().filter_map(|id| id.container.clone()).collect();
                    let mut ghosts: HashSet<PlacementId> = HashSet::new();
                    let mut sizes = HashMap::new();
                    for id in &wanted {
                        let node = doc.node(&id.node).unwrap();
                        let is_container = containers.contains(&id.node);
                        let is_new = !render.nodes.contains_key(id);
                        if is_new {
                            let v = {
                                let e = {
                                    let (state, id) = (&Rc::downgrade(state), id);
                                    let text = el("div").classes(&["gd_node_text"]);
                                    let inner = el("div").classes(&["gd_node"]).push(text);
                                    let node = el("div").classes(&["gd_node_wrap"]).push(inner);
                                    node.ref_on_with_options(
                                        "mousedown",
                                        EventListenerOptions::enable_prevent_default(),
                                        {
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
                                                        state.click_select(pc, &id);
                                                    } else if button == 2 {
                                                        state.click_extend(pc, &id);
                                                    }
                                                });
                                            }
                                        }
                                    );
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
                                    node.ref_on_with_options(
                                        "contextmenu",
                                        EventListenerOptions::enable_prevent_default(),
                                        |ev| {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                        }
                                    );
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
                                    node
                                };
                                let rect = HistPrim::new(pc, Rect4::default());
                                let opacity = HistPrim::new(pc, state.fade.get());
                                let links =
                                    vec![
                                        link!(
                                            (_pc = pc),
                                            (rect = rect.clone()),
                                            (),
                                            (e = e.weak(), state = Rc::downgrade(state), id = id.clone()) {
                                                write_rect(&e.upgrade()?, &rect.get());
                                                let state = state.upgrade()?;
                                                if state.focus_node().as_ref() == Some(&id.node) &&
                                                    state
                                                        .layout
                                                        .borrow()
                                                        .primary(&id.node)
                                                        .map(|p| &p.id == id)
                                                        .unwrap_or(false) {
                                                    place_overlay(&state, Some(&rect.get()));
                                                }
                                            }
                                        ),
                                        link!((_pc = pc), (opacity = opacity.clone()), (), (e = e.weak()) {
                                            write_opacity(&e.upgrade()?, opacity.get());
                                        }),
                                    ];
                                NodeView {
                                    el: e,
                                    rect: rect,
                                    opacity: opacity,
                                    secondary: false,
                                    fresh: true,
                                    _links: links,
                                }
                            };
                            nodes_el.ref_push(v.el.clone());
                            render.nodes.insert(id.clone(), v);
                        }
                        let e = &render.nodes[id].el;
                        e.ref_modify_classes(&[("gd_node_container", is_container)]);
                        let text_changed =
                            measured
                                .get(&id.node)
                                .map(|m| m.text != node.text || m.container != is_container)
                                .unwrap_or(true);
                        if is_new || text_changed {
                            let display = if node.text.is_empty() {
                                "\u{00a0}"
                            } else {
                                node.text.as_str()
                            };
                            node_text_el(e).unwrap().ref_text(display);
                        }
                        let ghost = doc.visible_parents(&id.node).first() != id.container.as_ref();
                        if !ghost {
                            if text_changed && e.raw().is_connected() {
                                let prev = measured.get(&id.node).cloned();
                                let (size, width, chrome) = {
                                    let (wrap_el, previous) = (e, prev.as_ref());
                                    let node_el = &node_inner_el(wrap_el).unwrap();
                                    let text_el = node_text_el(wrap_el).unwrap();
                                    remove_style(wrap_el, "width");
                                    remove_style(wrap_el, "height");
                                    remove_style(&text_el, "width");
                                    set_style(node_el, "width", "max-content");
                                    let (w0, h0) = offset_size(node_el);
                                    let (t0, _) = offset_size(&text_el);
                                    let chrome = w0 - t0;
                                    let mut chosen = None;
                                    if w0 > 3. * h0 && w0 > 0. {
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
                                    (size, chosen, chrome)
                                };
                                measured.insert(id.node.clone(), Measured {
                                    text: node.text.clone(),
                                    container: is_container,
                                    size: size,
                                    text_width: width,
                                    chrome: chrome,
                                    fit: None,
                                });
                            }
                            if let Some(m) = measured.get(&id.node) {
                                sizes.insert(id.node.clone(), m.size);
                            }
                        }
                        if ghost {
                            ghosts.insert(id.clone());
                        }
                        e.ref_modify_classes(&[("gd_node_ghost", ghost)]);
                    }
                    for id in &wanted {
                        let e = &render.nodes[id].el;
                        if let Some(m) = measured.get(&id.node) {
                            let text_el = node_text_el(e).unwrap();
                            let fit = m.fit.filter(|_| !ghosts.contains(id)).map(|(w, _)| w);
                            match fit.or(m.text_width) {
                                Some(w) => set_style(&text_el, "width", &px(w)),
                                None => remove_style(&text_el, "width"),
                            }
                        }
                    }
                    sizes
                };
                let previous = layout.borrow().clone();
                let config = {
                    let doc = state.doc.borrow();
                    let mut config = LayoutConfig::default();
                    config.flow = doc.flow;
                    let side_extent = if doc.flow.horizontal() {
                        height.get()
                    } else {
                        width.get()
                    };
                    config.max_rank_width = Some((side_extent as f64 - 2. * VIEW_MARGIN).max(200.));
                    config
                };
                let run = |titles: &HashMap<NodeId, NodeSize>| {
                    let doc = state.doc.borrow();
                    return grafdag_core::layout::layout(&doc, &sizes, titles, &config, Some(&previous));
                };
                let mut new_layout = run(&HashMap::new());
                if let Some(titles) = (|| {
                    let (state, layout, sizes, pad) = (&state, &new_layout, &sizes, config.container_pad);
                    let render = state.render.borrow();
                    let mut measured = state.measured.borrow_mut();
                    let mut out: HashMap<NodeId, NodeSize> = HashMap::new();
                    let mut changed = false;
                    for n in &layout.nodes {
                        if !n.container || n.ghost {
                            continue;
                        }
                        let Some(v) = render.nodes.get(&n.id) else {
                            continue;
                        };
                        let Some(m) = measured.get(&n.id.node).cloned() else {
                            continue;
                        };
                        let width = (n.rect.w - 2. * pad - m.chrome).max(1.);
                        let height = match m.fit {
                            Some((w, h)) if (w - width).abs() < 0.5 => h,
                            _ => {
                                let wrap_el = &v.el;
                                let node_el = &node_inner_el(wrap_el).unwrap();
                                let text_el = node_text_el(wrap_el).unwrap();
                                let (old_w, old_h) = (get_style(wrap_el, "width"), get_style(wrap_el, "height"));
                                remove_style(wrap_el, "width");
                                remove_style(wrap_el, "height");
                                set_style(node_el, "width", "max-content");
                                set_style(&text_el, "width", &px(width));
                                let (_, h) = offset_size(node_el);
                                remove_style(node_el, "width");
                                restore_style(wrap_el, "width", &old_w);
                                restore_style(wrap_el, "height", &old_h);
                                h
                            },
                        };
                        measured.get_mut(&n.id.node).unwrap().fit = Some((width, height));
                        let Some(size) = sizes.get(&n.id.node) else {
                            continue;
                        };
                        if (size.height - height).abs() > 0.5 {
                            changed = true;
                        }
                        out.insert(n.id.node.clone(), NodeSize {
                            width: size.width,
                            height: height,
                        });
                    }
                    if !changed {
                        return None;
                    }
                    return Some(out);
                })() {
                    new_layout = run(&titles);
                }
                layout.set(pc, Rc::new(new_layout));
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (layout = state.layout.clone()),
            (),
            (
                state = Rc::downgrade(state),
                nodes = nodes.clone(),
                svg = svg.clone(),
                paths = paths.clone(),
                labels = labels.clone()
            ) {
                let state = state.upgrade()?;
                {
                    let state = &state;
                    let layout_guard = layout.borrow();
                    let layout = &*layout_guard;
                    let (nodes_el, svg_el_, paths_el, labels_el) = (nodes, svg, paths, labels);
                    let doc = state.doc.borrow();
                    let mut render = state.render.borrow_mut();
                    let _ = nodes_el;
                    for n in &layout.nodes {
                        let Some(v) = render.nodes.get_mut(&n.id) else {
                            continue;
                        };
                        let e = v.el.clone();
                        set_style(&e, "z-index", &n.depth.to_string());
                        let fresh = v.fresh;
                        set_animated(pc, state, &v.rect, Rect4::from(n.rect), !fresh, |r| write_rect(&e, r));
                        let node = doc.node(&n.id.node);
                        v.secondary = is_secondary(&doc, node.map(|nn| nn.layers.as_slice()).unwrap_or(&[]));
                        e.ref_modify_classes(&[("gd_node_container", n.container)]);
                        e.ref_attr("data-node", &n.id.node.0);
                        let color =
                            node
                                .and_then(|nn| nn.layers.iter().find(|l| doc.layer_active(l)))
                                .and_then(|l| layer_color_index(&doc, l));
                        match color {
                            Some(i) => e.ref_attr("data-layer-color", &i.to_string()),
                            None => e.ref_remove_attr("data-layer-color"),
                        };
                    }
                    let w = layout.width;
                    let h = layout.height;
                    svg_el_.ref_attr("width", &format!("{}", w.max(1.)));
                    svg_el_.ref_attr("height", &format!("{}", h.max(1.)));
                    svg_el_.ref_attr("viewBox", &format!("0 0 {} {}", w.max(1.), h.max(1.)));
                    let mut wanted: HashSet<EdgeKey> = HashSet::new();
                    for e in &layout.edges {
                        if e.points.len() < 2 {
                            continue;
                        }
                        let key: EdgeKey = (e.id.clone(), e.source.clone(), e.dest.clone());
                        wanted.insert(key.clone());
                        if !render.edges.contains_key(&key) {
                            let old =
                                render
                                    .edges
                                    .keys()
                                    .find(
                                        |k| k.0 == key.0 && k.1.node == key.1.node && k.2.node == key.2.node &&
                                            !wanted.contains(*k),
                                    )
                                    .cloned();
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
                            hit.ref_on_with_options(
                                "contextmenu",
                                EventListenerOptions::enable_prevent_default(),
                                |ev| {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                }
                            );
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
                        let path_weak = v.el.weak();
                        let hit_weak = v.hit.weak();
                        let label_weak = v.label.as_ref().map(|l| l.weak());
                        v._links =
                            vec![
                                link!(
                                    (_pc = pc),
                                    (geom = v.geom.clone()),
                                    (),
                                    (path = path_weak.clone(), hit = hit_weak, label = label_weak.clone()) {
                                        let label = label.as_ref().and_then(|l| l.upgrade());
                                        write_edge_geom(
                                            &path.upgrade()?,
                                            &hit.upgrade()?,
                                            label.as_ref(),
                                            &geom.get()
                                        );
                                    }
                                ),
                                link!(
                                    (_pc = pc),
                                    (opacity = v.opacity.clone()),
                                    (),
                                    (path = path_weak, label = label_weak) {
                                        write_opacity(&path.upgrade()?, opacity.get());
                                        if let Some(l) = label.as_ref().and_then(|l| l.upgrade()) {
                                            write_opacity(&l, opacity.get());
                                        }
                                    }
                                ),
                            ];
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
                        set_animated(
                            pc,
                            state,
                            &v.geom,
                            geom,
                            same_shape && !fresh,
                            |g| write_edge_geom(&path, &hit, label.as_ref(), g),
                        );
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
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (start = state.sel_start.clone(), end = state.sel_end.clone(), layout = state.layout.clone()),
            (pan = state.pan.clone()),
            (state = Rc::downgrade(state)) {
                let _ = (start, end, layout, pan);
                let state = state.upgrade()?;
                state.follow_selection(pc);
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (peek = state.peek.clone(), layout = state.layout.clone()),
            (offset = state.peek_offset.clone()),
            (state = Rc::downgrade(state), last = std::cell::Cell::new(None)) {
                let _ = layout;
                let state = state.upgrade()?;
                let target = peek.get().and_then(|id| state.centering_offset(&id));
                if last.get() == Some(target) {
                    return None;
                }
                last.set(Some(target));
                match target {
                    Some(t) => {
                        if state.animate.get() {
                            offset.set_ease(&state.animator, t, TRANSITION_MS, ease);
                        } else {
                            offset.set(pc, t);
                        }
                    },
                    None => {
                        state.animator.cancel(offset);
                        offset.set(pc, Vec2::default());
                    },
                }
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (
                start = state.sel_start.clone(),
                end = state.sel_end.clone(),
                sel_edge = state.sel_edge.clone(),
                hover = state.hover.clone(),
                hover_edge = state.hover_edge.clone(),
                peek = state.peek.clone(),
            ),
            (),
            (state = Rc::downgrade(state)) {
                let changed =
                    start.get() != start.get_old() || end.get() != end.get_old() ||
                        sel_edge.get() != sel_edge.get_old() ||
                        hover.get() != hover.get_old() ||
                        hover_edge.get() != hover_edge.get_old() ||
                        peek.get() != peek.get_old();
                if !changed {
                    return None;
                }
                let state = state.upgrade()?;
                apply_selection(pc, &state, false);
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (pc = pc),
            (fade = state.fade.clone(), fade_secondary = state.fade_secondary.clone()),
            (),
            (state = Rc::downgrade(state)) {
                let _ = (fade, fade_secondary);
                let state = state.upgrade()?;
                apply_selection(pc, &state, true);
            }
        ),
    );
    canvas.ref_own(
        |_| link!(
            (_pc = pc),
            (zoom = state.zoom.clone(), pan = state.pan.clone(), offset = state.peek_offset.clone()),
            (),
            (world = world.clone()) {
                let Vec2(px, py) = pan.get();
                let off = offset.get();
                set_style(
                    world,
                    "transform",
                    &format!("translate({}px, {}px) scale({})", px + off.0, py + off.1, zoom.get())
                );
                set_style(world, "--gd-zoom", &zoom.get().to_string());
            }
        ),
    );
    canvas.ref_on_resize({
        let state = Rc::downgrade(state);
        move |_, w, h| {
            if let Some(state) = state.upgrade() {
                state.viewport.set((w, h));
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

#[derive(Clone, Debug, PartialEq, Default)]
pub struct EdgeGeom {
    pub label: Pt,
    pub points: Vec<Pt>,
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

pub type EdgeKey = (EdgeId, PlacementId, PlacementId);

pub struct EdgeView {
    _links: Vec<Link>,
    pub dest: NodeId,
    pub el: El,
    pub fresh: bool,
    pub geom: HistPrim<EdgeGeom>,
    pub hit: El,
    pub label: Option<El>,
    pub opacity: HistPrim<f64>,
    pub secondary: bool,
    pub source: NodeId,
}

fn get_style(e: &El, prop: &str) -> String {
    let raw = e.raw();
    let Some(h) = raw.dyn_ref::<HtmlElement>() else {
        return String::new();
    };
    return h.style().get_property_value(prop).unwrap_or_default();
}

fn is_secondary(doc: &grafdag_core::Document, layers: &[grafdag_core::LayerId]) -> bool {
    let Some(selected) = doc.selected_layer.as_ref().filter(|l| doc.layer_active(l)) else {
        return false;
    };
    if layers.is_empty() {
        return false;
    }
    return !layers.contains(selected);
}

fn layer_color_index(doc: &grafdag_core::Document, layer: &grafdag_core::LayerId) -> Option<usize> {
    return doc.layers.iter().position(|l| &l.id == layer).map(|i| i % 8);
}

fn make_overlay_button(
    state: &Rc<State>,
    icon: &str,
    title: &str,
    cb: impl Fn(&State, &mut ProcessingContext) + 'static,
) -> El {
    let b = super::toolbar::icon_button(state, icon, title, cb);
    b.ref_classes(&["gd_overlay_button"]);
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

fn node_inner_el(wrap: &El) -> Option<El> {
    return Some(el_from_raw(wrap.raw().first_element_child()?));
}

fn node_text_el(wrap: &El) -> Option<El> {
    return Some(el_from_raw(wrap.raw().first_element_child()?.first_element_child()?));
}

pub struct NodeView {
    _links: Vec<Link>,
    pub el: El,
    pub fresh: bool,
    pub opacity: HistPrim<f64>,
    pub rect: HistPrim<Rect4>,
    pub secondary: bool,
}

fn offset_size(e: &El) -> (f64, f64) {
    let h = e.raw().dyn_into::<HtmlElement>().unwrap();
    return (h.offset_width() as f64, h.offset_height() as f64);
}

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
    let inward = state.inward_direction();
    place_overlay_button(&ov.next, r, flow.screen_dir(if inward {
        Motion::Backward
    } else {
        Motion::Forward
    }));
}

fn place_overlay_button(button: &El, r: &Rect4, side: ScreenDir) {
    let [x, y, w, h] = r.0;
    let (name, ax, ay) = match side {
        ScreenDir::Right => ("right", x + w, y + h / 2.),
        ScreenDir::Left => ("left", x, y + h / 2.),
        ScreenDir::Down => ("bottom", x + w / 2., y + h),
        ScreenDir::Up => ("top", x + w / 2., y),
    };
    button.ref_attr("data-side", name);
    set_style(button, "left", &px(ax));
    set_style(button, "top", &px(ay));
}

fn px(v: f64) -> String {
    return format!("{}px", v);
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect4(pub [f64; 4]);

impl Add for Rect4 {
    type Output = Rect4;

    fn add(self, o: Rect4) -> Rect4 {
        return Rect4([self.0[0] + o.0[0], self.0[1] + o.0[1], self.0[2] + o.0[2], self.0[3] + o.0[3]]);
    }
}

impl From<Rect> for Rect4 {
    fn from(r: Rect) -> Self {
        return Rect4([r.x, r.y, r.w, r.h]);
    }
}

impl Mul<f64> for Rect4 {
    type Output = Rect4;

    fn mul(self, k: f64) -> Rect4 {
        return Rect4([self.0[0] * k, self.0[1] * k, self.0[2] * k, self.0[3] * k]);
    }
}

impl Sub for Rect4 {
    type Output = Rect4;

    fn sub(self, o: Rect4) -> Rect4 {
        return Rect4([self.0[0] - o.0[0], self.0[1] - o.0[1], self.0[2] - o.0[2], self.0[3] - o.0[3]]);
    }
}

pub fn remove_style(e: &El, prop: &str) {
    if let Some(h) = e.raw().dyn_ref::<HtmlElement>() {
        h.style().remove_property(prop).unwrap();
    }
}

fn restore_style(e: &El, prop: &str, value: &str) {
    if value.is_empty() {
        remove_style(e, prop);
    } else {
        set_style(e, prop, value);
    }
}

fn set_animated<
    T,
>(pc: &mut ProcessingContext, state: &State, prim: &HistPrim<T>, value: T, animate: bool, write: impl FnOnce(&T))
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

pub fn set_style(e: &El, prop: &str, value: &str) {
    if let Some(h) = e.raw().dyn_ref::<HtmlElement>() {
        h.style().set_property(prop, value).unwrap();
    } else if let Some(s) = e.raw().dyn_ref::<web_sys::SvgElement>() {
        s.style().set_property(prop, value).unwrap();
    }
}

pub fn svg_el(tag: &str) -> El {
    return el_from_raw(document().create_element_ns(Some("http://www.w3.org/2000/svg"), tag).unwrap());
}

fn write_edge_geom(path: &El, hit: &El, label: Option<&El>, g: &EdgeGeom) {
    let d = {
        let points = &g.points;
        let mut out = String::new();
        for (i, p) in points.iter().enumerate() {
            if i == 0 {
                out.push_str(&format!("M{:.1} {:.1}", p.x, p.y));
            } else {
                out.push_str(&format!(" L{:.1} {:.1}", p.x, p.y));
            }
        }
        out
    };
    path.ref_attr("d", &d);
    hit.ref_attr("d", &d);
    if let Some(l) = label {
        set_style(l, "left", &px(g.label.x));
        set_style(l, "top", &px(g.label.y));
    }
}

fn write_opacity(e: &El, v: f64) {
    set_style(e, "opacity", &format!("{:.3}", v));
}

fn write_rect(e: &El, r: &Rect4) {
    set_style(e, "left", &px(r.0[0]));
    set_style(e, "top", &px(r.0[1]));
    set_style(e, "width", &px(r.0[2]));
    set_style(e, "height", &px(r.0[3]));
}
