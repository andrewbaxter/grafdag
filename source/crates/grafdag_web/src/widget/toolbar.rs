use {
    super::state::{
        SearchTarget,
        State,
    },
    lunk::{
        link,
        ProcessingContext,
    },
    rooting::{
        el,
        El,
    },
    std::rc::Rc,
};

pub fn icon_button(state: &Rc<State>, icon: &str, title: &str, cb: impl Fn(&State, &mut ProcessingContext) + 'static) -> El {
    let weak = Rc::downgrade(state);
    return el("button")
        .classes(&["gd_button"])
        .attr("title", title)
        .attr("type", "button")
        .push(el("span").classes(&["gd_icon"]).text(icon))
        .on("click", move |_| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            state.eg.event(|pc| cb(&state, pc));
        });
}

fn separator() -> El {
    return el("span").classes(&["gd_toolbar_sep"]);
}

pub fn build_toolbar(pc: &mut ProcessingContext, state: &Rc<State>) -> El {
    let undo = icon_button(state, "undo", "Undo (z)", |s, pc| s.undo(pc));
    let redo = icon_button(state, "redo", "Redo (Z)", |s, pc| s.redo(pc));
    undo.ref_own(|undo| link!((_pc = pc), (can = state.can_undo.clone()), (), (undo = undo.weak()) {
        undo.upgrade()?.ref_modify_classes(&[("gd_button_disabled", !can.get())]);
    }));
    redo.ref_own(|redo| link!((_pc = pc), (can = state.can_redo.clone()), (), (redo = redo.weak()) {
        redo.upgrade()?.ref_modify_classes(&[("gd_button_disabled", !can.get())]);
    }));
    let pair_buttons = vec![
        icon_button(state, "link", "Link start to end (l)", |s, pc| s.cmd_link(pc)),
    ];
    for b in &pair_buttons {
        // A set anchor implies a primary node, so it alone means a pair
        b.ref_own(|b| link!((_pc = pc), (start = state.sel_start.clone()), (), (b = b.weak()) {
            b.upgrade()?.ref_modify_classes(&[("gd_button_disabled", start.get().is_none())]);
        }));
    }
    let edge_buttons = vec![
        icon_button(state, "edit_note", "Edit selected link (L)", |s, pc| s.cmd_edit_link(pc)),
        icon_button(state, "link_off", "Delete selected link (u)", |s, pc| s.cmd_unlink(pc)),
        icon_button(state, "swap_vert", "Reverse selected link (r)", |s, pc| s.cmd_reverse(pc)),
    ];
    for b in &edge_buttons {
        b.ref_own(|b| link!((_pc = pc), (edge = state.sel_edge.clone()), (), (b = b.weak()) {
            b.upgrade()?.ref_modify_classes(&[("gd_button_disabled", edge.get().is_none())]);
        }));
    }
    let focus_buttons = vec![
        icon_button(state, "edit", "Edit node (e)", |s, pc| s.cmd_edit(pc)),
        icon_button(state, "delete", "Delete node (Delete)", |s, pc| s.cmd_delete(pc)),
    ];
    for b in &focus_buttons {
        b.ref_own(|b| link!((_pc = pc), (end = state.sel_end.clone()), (), (b = b.weak()) {
            b.upgrade()?.ref_modify_classes(&[("gd_button_disabled", end.get().is_none())]);
        }));
    }
    let zoom = el("span").classes(&["gd_zoom"]).attr("title", "Zoom");
    zoom.ref_own(|z| link!((_pc = pc), (zoom = state.zoom.clone()), (), (z = z.weak()) {
        z.upgrade()?.ref_text(&format!("{}%", (zoom.get() * 100.).round()));
    }));
    let status = el("span").classes(&["gd_status"]);
    status.ref_own(|status| link!((_pc = pc), (text = state.status.clone()), (), (status = status.weak()) {
        status.upgrade()?.ref_text(&text.get());
    }));
    let mut items = vec![undo, redo, separator()];
    items.push(icon_button(state, "zoom_in", "Zoom in (+)", |s, pc| s.cmd_zoom(pc, 1.2, None)));
    items.push(icon_button(state, "zoom_out", "Zoom out (-)", |s, pc| s.cmd_zoom(pc, 1. / 1.2, None)));
    items.push(icon_button(state, "fit_screen", "Fit to view (0)", |s, pc| s.cmd_fit(pc)));
    items.push(
        icon_button(state, "rotate_left", "Rotate layout counterclockwise", |s, pc| s.cmd_rotate_flow(pc, false)),
    );
    items.push(icon_button(state, "rotate_right", "Rotate layout clockwise", |s, pc| s.cmd_rotate_flow(pc, true)));
    items.push(zoom);
    items.push(separator());
    items.push(icon_button(state, "add", "New unlinked node (N)", |s, pc| s.cmd_new_island(pc)));
    items.extend(focus_buttons);
    items.push(separator());
    items.extend(pair_buttons);
    items.extend(edge_buttons);
    items.push(separator());
    items.push(icon_button(state, "search", "Search (/)", |s, pc| s.cmd_search(pc, SearchTarget::Replace)));
    items.push(icon_button(state, "view_sidebar", "Toggle side panel (Tab)", |s, pc| s.cmd_toggle_panel(pc)));
    items.push(el("span").classes(&["gd_toolbar_spacer"]));
    items.push(status);
    return el("div").classes(&["gd_toolbar"]).extend(items);
}
