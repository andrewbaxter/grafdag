use {
    grafdag_core::{
        Action,
        CoalesceKey,
        DefaultTrue,
        Layer,
        LayerId,
        NodeId,
        delete_layer_actions,
    },
    lunk::{
        ProcessingContext,
        link,
    },
    rooting::{
        El,
        el,
    },
    std::{
        cell::Cell,
        rc::Rc,
    },
    super::{
        state::{
            Mode,
            SearchTarget,
            State,
        },
        toolbar::icon_button,
    },
    wasm_bindgen::JsCast,
    web_sys::{
        HtmlElement,
        HtmlInputElement,
        HtmlSelectElement,
        HtmlTextAreaElement,
        KeyboardEvent,
        ScrollIntoViewOptions,
        ScrollLogicalPosition,
    },
};

pub fn build_panel(pc: &mut ProcessingContext, state: &Rc<State>) -> El {
    let content = el("div").classes(&["gd_panel_content"]);
    content.ref_own(
        |c| link!(
            (pc = pc),
            (mode = state.mode.clone()),
            (peek = state.peek.clone()),
            (c = c.weak(), state = Rc::downgrade(state)) {
                let c = c.upgrade()?;
                let state = state.upgrade()?;
                let searching = matches!(mode.get(), Mode::Search(_));
                let body = match mode.get() {
                    Mode::Layers => {
                        let state = &state;
                        let list = el("div").classes(&["gd_layer_list"]);
                        list.ref_own(
                            |l| link!(
                                (_pc = pc),
                                (version = state.doc_version.clone()),
                                (),
                                (l = l.weak(), state = Rc::downgrade(state)) {
                                    let _ = version;
                                    let l = l.upgrade()?;
                                    let state = state.upgrade()?;
                                    let rows = {
                                        let doc = state.doc.borrow();
                                        doc.layers.iter().enumerate().map(|(i, layer)| {
                                            let state = &state;
                                            let selected = doc.selected_layer.as_ref() == Some(&layer.id);
                                            let color = i % 8;
                                            let id = layer.id.clone();
                                            let checkbox =
                                                el("input")
                                                    .classes(&["gd_checkbox"])
                                                    .attr("type", "checkbox")
                                                    .attr("title", "Show layer",);
                                            if layer.active.0 {
                                                checkbox.ref_attr("checked", "");
                                            }
                                            checkbox.ref_on("change", {
                                                let weak = Rc::downgrade(state);
                                                let id = id.clone();
                                                move |ev| {
                                                    let Some(state) = weak.upgrade() else {
                                                        return;
                                                    };
                                                    let checked = input_checked(ev);
                                                    let layer = state.doc.borrow().layer(&id).cloned();
                                                    let Some(mut layer) = layer else {
                                                        return;
                                                    };
                                                    layer.active = DefaultTrue(checked);
                                                    state.eg.event(|pc| {
                                                        state.commit(pc, vec![Action::LayerModify(layer)], None);
                                                    });
                                                }
                                            });
                                            let name =
                                                el("span")
                                                    .classes(&["gd_layer_name"])
                                                    .text(&layer.name)
                                                    .attr("title", "Make current layer",);
                                            name.ref_on("click", {
                                                let weak = Rc::downgrade(state);
                                                let id = id.clone();
                                                move |_| {
                                                    let Some(state) = weak.upgrade() else {
                                                        return;
                                                    };
                                                    let current = state.doc.borrow().selected_layer.clone();
                                                    let new = if current.as_ref() == Some(&id) {
                                                        None
                                                    } else {
                                                        Some(id.clone())
                                                    };
                                                    state.eg.event(|pc| {
                                                        state.commit(pc, vec![Action::SelectLayer(new)], None);
                                                    });
                                                }
                                            });
                                            let delete = icon_button(state, "close", "Delete layer", {
                                                let id = id.clone();
                                                move |state, pc| {
                                                    let actions = delete_layer_actions(&state.doc.borrow(), &id);
                                                    state.commit(pc, actions, None);
                                                }
                                            });
                                            let swatch =
                                                el("span")
                                                    .classes(&["gd_layer_swatch"])
                                                    .attr("data-layer-color", &color.to_string());
                                            let row =
                                                el("div")
                                                    .classes(&["gd_layer_row"])
                                                    .extend(vec![checkbox, swatch, name, delete]);
                                            row.ref_modify_classes(&[("gd_layer_row_selected", selected)]);
                                            row
                                        }).collect::<Vec<_>>()
                                    };
                                    l.ref_clear();
                                    if rows.is_empty() {
                                        l.ref_push(
                                            el("div")
                                                .classes(&["gd_panel_hint"])
                                                .text("No layers. Nodes without layers are always shown.")
                                        );
                                    }
                                    l.ref_extend(rows);
                                }
                            ),
                        );
                        let new_input =
                            el("input")
                                .classes(&["gd_input"])
                                .attr("type", "text")
                                .attr("placeholder", "New layer name");
                        let add = {
                            let input = new_input.weak();
                            text_button(state, "Add", move |state, pc| {
                                let Some(input) = input.upgrade() else {
                                    return;
                                };
                                let raw = input.raw().dyn_into::<HtmlInputElement>().unwrap();
                                let name = raw.value().trim().to_string();
                                if name.is_empty() {
                                    return;
                                }
                                raw.set_value("");
                                let layer = {
                                    let doc = state.doc.borrow();
                                    Layer {
                                        id: doc.new_layer_id(),
                                        name: name,
                                        active: DefaultTrue(true),
                                    }
                                };
                                state.commit(pc, vec![Action::LayerCreate {
                                    layer: layer,
                                    index: None,
                                }], None);
                            })
                        };
                        new_input.ref_on("keydown", {
                            let add = add.weak();
                            move |ev| {
                                let Some(kev) = ev.dyn_ref::<KeyboardEvent>() else {
                                    return;
                                };
                                if kev.key() == "Enter" {
                                    if let Some(add) = add.upgrade() {
                                        add.raw().dyn_into::<HtmlElement>().unwrap().click();
                                    }
                                }
                            }
                        });
                        el("div")
                            .classes(&["gd_layers"])
                            .extend(
                                vec![
                                    heading("Layers"),
                                    list,
                                    el("div").classes(&["gd_row"]).push(new_input).push(add),
                                    el("div")
                                        .classes(&["gd_panel_hint"])
                                        .text(
                                            "Layers color node outlines and links. Check to show a layer, click the name to make it the current layer (new nodes and links go there, and layout prioritizes it).",
                                        ),
                                    heading("Fading"),
                                    slider(state, "Unselected", &state.fade),
                                    slider(state, "Outside current layer", &state.fade_secondary),
                                ],
                            )
                    },
                    Mode::EditNode(id) => {
                        let state = &state;
                        let id = &id;
                        let textarea =
                            el("textarea")
                                .classes(&["gd_textarea"])
                                .attr("rows", "4")
                                .attr("placeholder", "Node text");
                        textarea.ref_own(
                            |t| link!(
                                (_pc = pc),
                                (version = state.doc_version.clone()),
                                (),
                                (t = t.weak(), state = Rc::downgrade(state), id = id.clone()) {
                                    let _ = version;
                                    let t = t.upgrade()?;
                                    let state = state.upgrade()?;
                                    let text = state.doc.borrow().node(id)?.text.clone();
                                    let raw = t.raw().dyn_into::<HtmlTextAreaElement>().unwrap();
                                    if raw.value() != text {
                                        raw.set_value(&text);
                                    }
                                }
                            ),
                        );
                        textarea.ref_on("input", {
                            let weak = Rc::downgrade(state);
                            let id = id.clone();
                            move |ev| {
                                let Some(state) = weak.upgrade() else {
                                    return;
                                };
                                let value = input_value(ev);
                                let node = state.doc.borrow().node(&id).cloned();
                                let Some(mut node) = node else {
                                    return;
                                };
                                if node.text == value {
                                    return;
                                }
                                node.text = value;
                                state.eg.event(|pc| {
                                    state.commit(pc, vec![Action::NodeModify(node)], Some(CoalesceKey {
                                        target: id.0.clone(),
                                        field: "text".into(),
                                    }));
                                });
                            }
                        });
                        *state.focus_request.borrow_mut() = Some(textarea.clone());
                        let dynamic = el("div");
                        dynamic.ref_own(
                            |d| link!(
                                (_pc = pc),
                                (version = state.doc_version.clone()),
                                (),
                                (d = d.weak(), state = Rc::downgrade(state), id = id.clone()) {
                                    let _ = version;
                                    let d = d.upgrade()?;
                                    let state = state.upgrade()?;
                                    d.ref_clear();
                                    d.ref_extend((|| -> Vec<El> {
                                        let state = &state;
                                        let doc = state.doc.borrow();
                                        let Some(node) = doc.node(id) else {
                                            return vec![];
                                        };
                                        let mut out = vec![];
                                        if !doc.layers.is_empty() {
                                            out.push(el("div").classes(&["gd_field_label"]).text("Layers"));
                                            for layer in &doc.layers {
                                                let checkbox =
                                                    el("input").classes(&["gd_checkbox"]).attr("type", "checkbox");
                                                if node.layers.contains(&layer.id) {
                                                    checkbox.ref_attr("checked", "");
                                                }
                                                checkbox.ref_on("change", {
                                                    let weak = Rc::downgrade(state);
                                                    let id = id.clone();
                                                    let layer_id = layer.id.clone();
                                                    move |ev| {
                                                        let Some(state) = weak.upgrade() else {
                                                            return;
                                                        };
                                                        let checked = input_checked(ev);
                                                        let node = state.doc.borrow().node(&id).cloned();
                                                        let Some(mut node) = node else {
                                                            return;
                                                        };
                                                        node.layers.retain(|l| l != &layer_id);
                                                        if checked {
                                                            node.layers.push(layer_id.clone());
                                                        }
                                                        state.eg.event(|pc| {
                                                            state.commit(pc, vec![Action::NodeModify(node)], None);
                                                        });
                                                    }
                                                });
                                                out.push(
                                                    el("label")
                                                        .classes(&["gd_check_row"])
                                                        .push(checkbox)
                                                        .push(el("span").text(&layer.name),),
                                                );
                                            }
                                        }
                                        out.push(
                                            el("div").classes(&["gd_field_label"]).text("Parents (drawn inside)")
                                        );
                                        for p in &node.parents {
                                            let remove = icon_button(state, "close", "Remove parent", {
                                                let id = id.clone();
                                                let p = p.clone();
                                                move |state, pc| {
                                                    let node = state.doc.borrow().node(&id).cloned();
                                                    let Some(mut node) = node else {
                                                        return;
                                                    };
                                                    node.parents.retain(|x| x != &p);
                                                    state.commit(pc, vec![Action::NodeModify(node)], None);
                                                }
                                            });
                                            out.push(
                                                el("div")
                                                    .classes(&["gd_parent_row"])
                                                    .push(
                                                        el("span")
                                                            .classes(&["gd_parent_name"])
                                                            .text(&node_label(state, p))
                                                    )
                                                    .push(remove),
                                            );
                                        }
                                        let select = el("select").classes(&["gd_select"]);
                                        select.ref_push(el("option").attr("value", "").text("Add parent…"));
                                        for other in &doc.nodes {
                                            if other.id == *id || node.parents.contains(&other.id) {
                                                continue;
                                            }
                                            select.ref_push(
                                                el("option")
                                                    .attr("value", &other.id.0)
                                                    .text(&node_label(state, &other.id))
                                            );
                                        }
                                        select.ref_on("change", {
                                            let weak = Rc::downgrade(state);
                                            let id = id.clone();
                                            move |ev| {
                                                let Some(state) = weak.upgrade() else {
                                                    return;
                                                };
                                                let value = input_value(ev);
                                                if value.is_empty() {
                                                    return;
                                                }
                                                let node = state.doc.borrow().node(&id).cloned();
                                                let Some(mut node) = node else {
                                                    return;
                                                };
                                                node.parents.push(NodeId(value));
                                                state.eg.event(|pc| {
                                                    state.commit(pc, vec![Action::NodeModify(node)], None);
                                                });
                                            }
                                        });
                                        out.push(select);
                                        return out;
                                    })());
                                }
                            ),
                        );
                        let delete = text_button(state, "Delete node", {
                            let id = id.clone();
                            move |state, pc| {
                                state.cmd_delete_node(pc, &id);
                                state.mode.set(pc, Mode::Layers);
                            }
                        });
                        closable_pane(
                            state,
                            "gd_editor",
                            &format!("Node {}", id.0),
                            vec![textarea, dynamic, el("div").classes(&["gd_row"]).push(delete)],
                        )
                    },
                    Mode::EditEdge(id) => {
                        let state = &state;
                        let id = &id;
                        let input =
                            el("input").classes(&["gd_input"]).attr("type", "text").attr("placeholder", "Link text");
                        input.ref_own(
                            |t| link!(
                                (_pc = pc),
                                (version = state.doc_version.clone()),
                                (),
                                (t = t.weak(), state = Rc::downgrade(state), id = id.clone()) {
                                    let _ = version;
                                    let t = t.upgrade()?;
                                    let state = state.upgrade()?;
                                    let text = state.doc.borrow().edge(id)?.text.clone();
                                    let raw = t.raw().dyn_into::<HtmlInputElement>().unwrap();
                                    if raw.value() != text {
                                        raw.set_value(&text);
                                    }
                                }
                            ),
                        );
                        input.ref_on("input", {
                            let weak = Rc::downgrade(state);
                            let id = id.clone();
                            move |ev| {
                                let Some(state) = weak.upgrade() else {
                                    return;
                                };
                                let value = input_value(ev);
                                let edge = state.doc.borrow().edge(&id).cloned();
                                let Some(mut edge) = edge else {
                                    return;
                                };
                                if edge.text == value {
                                    return;
                                }
                                edge.text = value;
                                state.eg.event(|pc| {
                                    state.commit(pc, vec![Action::EdgeModify(edge)], Some(CoalesceKey {
                                        target: id.0.clone(),
                                        field: "text".into(),
                                    }));
                                });
                            }
                        });
                        *state.focus_request.borrow_mut() = Some(input.clone());
                        let dynamic = el("div");
                        dynamic.ref_own(
                            |d| link!(
                                (_pc = pc),
                                (version = state.doc_version.clone()),
                                (),
                                (d = d.weak(), state = Rc::downgrade(state), id = id.clone()) {
                                    let _ = version;
                                    let d = d.upgrade()?;
                                    let state = state.upgrade()?;
                                    d.ref_clear();
                                    d.ref_extend((|| -> Vec<El> {
                                        let state = &state;
                                        let doc = state.doc.borrow();
                                        let Some(edge) = doc.edge(id) else {
                                            return vec![];
                                        };
                                        let mut out = vec![];
                                        out.push(
                                            el("div")
                                                .classes(&["gd_panel_hint"])
                                                .text(
                                                    &format!(
                                                        "{} \u{2192} {}",
                                                        node_label(state, &edge.source),
                                                        node_label(state, &edge.dest)
                                                    ),
                                                ),
                                        );
                                        out.push(el("div").classes(&["gd_field_label"]).text("Layer"));
                                        let select = el("select").classes(&["gd_select"]);
                                        select.ref_push(el("option").attr("value", "").text("(none)"));
                                        for layer in &doc.layers {
                                            let opt = el("option").attr("value", &layer.id.0).text(&layer.name);
                                            if edge.layer.as_ref() == Some(&layer.id) {
                                                opt.ref_attr("selected", "");
                                            }
                                            select.ref_push(opt);
                                        }
                                        select.ref_on("change", {
                                            let weak = Rc::downgrade(state);
                                            let id = id.clone();
                                            move |ev| {
                                                let Some(state) = weak.upgrade() else {
                                                    return;
                                                };
                                                let value = input_value(ev);
                                                let edge = state.doc.borrow().edge(&id).cloned();
                                                let Some(mut edge) = edge else {
                                                    return;
                                                };
                                                edge.layer = if value.is_empty() {
                                                    None
                                                } else {
                                                    Some(LayerId(value))
                                                };
                                                state.eg.event(|pc| {
                                                    state.commit(pc, vec![Action::EdgeModify(edge)], None);
                                                });
                                            }
                                        });
                                        out.push(select);
                                        return out;
                                    })());
                                }
                            ),
                        );
                        let reverse = text_button(state, "Reverse", {
                            let id = id.clone();
                            move |state, pc| {
                                let edge = state.doc.borrow().edge(&id).cloned();
                                let Some(mut edge) = edge else {
                                    return;
                                };
                                std::mem::swap(&mut edge.source, &mut edge.dest);
                                state.commit(pc, vec![Action::EdgeModify(edge)], None);
                            }
                        });
                        let delete = text_button(state, "Delete link", {
                            let id = id.clone();
                            move |state, pc| {
                                state.commit(pc, vec![Action::EdgeDelete(id.clone())], None);
                                state.mode.set(pc, Mode::Layers);
                            }
                        });
                        closable_pane(
                            state,
                            "gd_editor",
                            &format!("Link {}", id.0),
                            vec![input, dynamic, el("div").classes(&["gd_row"]).push(reverse).push(delete)],
                        )
                    },
                    Mode::Search(target) => {
                        let state = &state;
                        let input =
                            el("input")
                                .classes(&["gd_input"])
                                .attr("type", "text")
                                .attr("placeholder", "Search nodes");
                        input.ref_on("input", {
                            let weak = Rc::downgrade(state);
                            move |ev| {
                                let Some(state) = weak.upgrade() else {
                                    return;
                                };
                                let value = input_value(ev);
                                state.eg.event(|pc| state.cmd_search_query(pc, value));
                            }
                        });
                        input.ref_on("keydown", {
                            let weak = Rc::downgrade(state);
                            move |ev| {
                                let Some(state) = weak.upgrade() else {
                                    return;
                                };
                                let Some(kev) = ev.dyn_ref::<KeyboardEvent>() else {
                                    return;
                                };
                                match kev.key().as_str() {
                                    "ArrowDown" => {
                                        ev.prevent_default();
                                        state.eg.event(|pc| state.cmd_search_move(pc, 1));
                                    },
                                    "ArrowUp" => {
                                        ev.prevent_default();
                                        state.eg.event(|pc| state.cmd_search_move(pc, -1));
                                    },
                                    "Enter" => {
                                        ev.prevent_default();
                                        state.eg.event(|pc| state.cmd_search_accept(pc));
                                    },
                                    _ => { },
                                }
                            }
                        });
                        *state.focus_request.borrow_mut() = Some(input.clone());
                        let results = el("div").classes(&["gd_search_results"]);
                        results.ref_own(
                            |r| link!(
                                (pc = pc),
                                (
                                    query = state.search_query.clone(),
                                    index = state.search_index.clone(),
                                    version = state.doc_version.clone(),
                                ),
                                (peek = state.peek.clone()),
                                (r = r.weak(), state = Rc::downgrade(state), built = Cell::new(false)) {
                                    let r = r.upgrade()?;
                                    let state = state.upgrade()?;
                                    let results = state.search_results();
                                    let rebuild =
                                        !built.get() || query.get() != query.get_old() ||
                                            version.get() != version.get_old();
                                    if rebuild {
                                        built.set(true);
                                        let rows: Vec<El> = results.iter().enumerate().map(|(i, (id, text))| {
                                            let state = &state;
                                            let label = if text.is_empty() {
                                                format!("({})", id.0)
                                            } else {
                                                text.lines().next().unwrap_or("").to_string()
                                            };
                                            let row = el("div").classes(&["gd_search_row"]).text(&label);
                                            row.ref_on("click", {
                                                let weak = Rc::downgrade(state);
                                                move |_| {
                                                    let Some(state) = weak.upgrade() else {
                                                        return;
                                                    };
                                                    state.eg.event(|pc| {
                                                        state.search_index.set(pc, i);
                                                        state.cmd_search_accept(pc);
                                                    });
                                                }
                                            });
                                            for (event, entering) in [("mouseenter", true), ("mouseleave", false)] {
                                                row.ref_on(event, {
                                                    let weak = Rc::downgrade(state);
                                                    let id = id.clone();
                                                    move |_| {
                                                        let Some(state) = weak.upgrade() else {
                                                            return;
                                                        };
                                                        state.eg.event(|pc| state.cmd_search_hover(pc, &id, entering));
                                                    }
                                                });
                                            }
                                            row
                                        }).collect();
                                        r.ref_clear();
                                        r.ref_extend(rows);
                                    }
                                    peek.set(pc, results.get(index.get()).map(|(id, _)| id.clone()));
                                    let children = r.raw().children();
                                    for i in 0 .. children.length() {
                                        let Some(row) = children.item(i) else {
                                            continue;
                                        };
                                        let active = i as usize == index.get();
                                        row.class_list().toggle_with_force("gd_search_row_active", active).ok();
                                        if active {
                                            let opts = ScrollIntoViewOptions::new();
                                            opts.set_block(ScrollLogicalPosition::Nearest);
                                            row.scroll_into_view_with_scroll_into_view_options(&opts);
                                        }
                                    }
                                }
                            ),
                        );
                        let title = match target {
                            SearchTarget::Replace => "Search (select node)",
                            SearchTarget::Extend => "Search (select second node)",
                        };
                        closable_pane(state, "gd_search", title, vec![input, results])
                    },
                };
                c.ref_clear();
                c.ref_push(body);
                c.ref_modify_classes(&[("gd_panel_content_fill", searching)]);
                peek.set(pc, if searching {
                    state.search_current()
                } else {
                    None
                });
                let f = state.focus_request.borrow_mut().take();
                if let Some(f) = f {
                    if let Ok(h) = f.raw().dyn_into::<HtmlElement>() {
                        h.focus().ok();
                    }
                }
            }
        ),
    );
    let panel = el("div").classes(&["gd_panel"]).push(content);
    panel.ref_own(|p| link!((_pc = pc), (open = state.panel_open.clone()), (), (p = p.weak()) {
        p.upgrade()?.ref_modify_classes(&[("gd_panel_hidden", !open.get())]);
    }));
    return panel;
}

fn closable_pane(state: &Rc<State>, class: &str, title: &str, body: Vec<El>) -> El {
    let close = icon_button(state, "close", "Close (Escape)", |state, pc| state.mode.set(pc, Mode::Layers));
    let head = el("div").classes(&["gd_panel_head"]).push(heading(title)).push(close);
    return el("div").classes(&[class]).push(head).extend(body);
}

fn heading(text: &str) -> El {
    return el("div").classes(&["gd_panel_heading"]).text(text);
}

fn input_checked(ev: &web_sys::Event) -> bool {
    return ev
        .target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false);
}

fn input_value(ev: &web_sys::Event) -> String {
    let Some(t) = ev.target() else {
        return "".into();
    };
    if let Some(i) = t.dyn_ref::<HtmlInputElement>() {
        return i.value();
    }
    if let Some(i) = t.dyn_ref::<HtmlTextAreaElement>() {
        return i.value();
    }
    if let Some(i) = t.dyn_ref::<HtmlSelectElement>() {
        return i.value();
    }
    return "".into();
}

fn node_label(state: &State, id: &NodeId) -> String {
    let doc = state.doc.borrow();
    let text = doc.node(id).map(|n| n.text.clone()).unwrap_or_default();
    let text = text.lines().next().unwrap_or("").to_string();
    if text.is_empty() {
        return format!("({})", id.0);
    }
    return text;
}

fn slider(state: &Rc<State>, label: &str, prim: &lunk::HistPrim<f64>) -> El {
    let input =
        el("input")
            .classes(&["gd_slider"])
            .attr("type", "range")
            .attr("min", "0")
            .attr("max", "1")
            .attr("step", "0.05");
    input.ref_attr("value", &prim.get().to_string());
    input.ref_on("input", {
        let weak = Rc::downgrade(state);
        let prim = prim.clone();
        move |ev| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Ok(value) = input_value(ev).parse::<f64>() else {
                return;
            };
            state.eg.event(|pc| {
                prim.set(pc, value);
            });
        }
    });
    return el("label")
        .classes(&["gd_slider_row"])
        .push(el("span").classes(&["gd_slider_label"]).text(label))
        .push(input);
}

fn text_button(state: &Rc<State>, label: &str, cb: impl Fn(&State, &mut lunk::ProcessingContext) + 'static) -> El {
    let weak = Rc::downgrade(state);
    return el("button").classes(&["gd_text_button"]).attr("type", "button").text(label).on("click", move |_| {
        let Some(state) = weak.upgrade() else {
            return;
        };
        state.eg.event(|pc| cb(&state, pc));
    });
}
