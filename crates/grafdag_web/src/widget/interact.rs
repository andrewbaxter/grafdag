//! Keyboard and mouse handling.
use {
    super::{
        commands::Dir,
        state::{
            Mode,
            SearchTarget,
            State,
        },
    },
    gloo_events::EventListener,
    gloo_utils::{
        document,
        window,
    },
    rooting::El,
    std::{
        cell::Cell,
        rc::{
            Rc,
            Weak,
        },
    },
    wasm_bindgen::JsCast,
    web_sys::{
        KeyboardEvent,
        MouseEvent,
        WheelEvent,
    },
};

fn is_text_input(ev: &web_sys::Event) -> bool {
    let Some(target) = ev.target() else {
        return false;
    };
    let Some(el) = target.dyn_ref::<web_sys::Element>() else {
        return false;
    };
    let tag = el.tag_name().to_lowercase();
    return tag == "input" || tag == "textarea" || tag == "select";
}

/// Handle a key press outside text inputs. Returns true if handled.
pub fn handle_key(state: &Rc<State>, ev: &KeyboardEvent) -> bool {
    let key = ev.key();
    let ctrl = ev.ctrl_key() || ev.meta_key();
    let shift = ev.shift_key();
    let mut handled = true;
    state.eg.event(|pc| {
        if ctrl {
            match key.as_str() {
                "z" => state.undo(pc),
                "Z" | "y" => state.redo(pc),
                "=" | "+" => state.cmd_zoom(pc, 1.2, None),
                "-" => state.cmd_zoom(pc, 1. / 1.2, None),
                "0" => state.cmd_fit(pc),
                _ => handled = false,
            }
            return;
        }
        match key.as_str() {
            "ArrowLeft" => state.cmd_sibling(pc, Dir::Left),
            "ArrowRight" => state.cmd_sibling(pc, Dir::Right),
            "ArrowDown" if shift => state.cmd_select_next(pc),
            "ArrowUp" if shift => state.cmd_select_prev(pc),
            "ArrowDown" => state.cmd_forward(pc),
            "ArrowUp" => state.cmd_backward(pc),
            "Tab" => state.cmd_island(pc, !shift),
            "Escape" => state.cmd_escape(pc),
            "f" => state.cmd_flip(pc),
            "l" => state.cmd_link(pc),
            "u" => state.cmd_unlink(pc),
            "r" => state.cmd_reverse(pc),
            "n" => state.cmd_new_next(pc),
            "s" => state.cmd_new_sibling(pc),
            "Delete" | "Backspace" => state.cmd_delete(pc),
            "e" | "Enter" => state.cmd_edit(pc),
            "E" => state.cmd_edit_start(pc),
            "L" => state.cmd_edit_link(pc),
            "/" => state.cmd_search(pc, SearchTarget::Start),
            "?" => state.cmd_search(pc, SearchTarget::End),
            "+" | "=" => state.cmd_zoom(pc, 1.2, None),
            "-" => state.cmd_zoom(pc, 1. / 1.2, None),
            "0" => state.cmd_fit(pc),
            "z" => state.undo(pc),
            "Z" => state.redo(pc),
            _ => handled = false,
        }
    });
    return handled;
}

/// Attach global (document level) keyboard handling to the widget root.
pub fn attach(root: &El, state: &Rc<State>) {
    let weak = Rc::downgrade(state);
    root.ref_own(move |_| {
        EventListener::new(&document(), "keydown", move |ev| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(kev) = ev.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if is_text_input(ev) {
                if kev.key() == "Escape" {
                    if let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        t.blur().ok();
                    }
                    state.eg.event(|pc| {
                        state.mode.set(pc, Mode::Layers);
                    });
                    ev.prevent_default();
                }
                return;
            }
            if handle_key(&state, kev) {
                ev.prevent_default();
            }
        })
    });
}

/// Attach mouse handling to the canvas: middle drag pans, wheel zooms, clicks
/// on empty space clear the selection.
pub fn attach_canvas(canvas: &El, state: &Rc<State>) {
    let weak: Weak<State> = Rc::downgrade(state);
    // Pan drag state: (start mouse x, y, start pan x, y)
    let drag: Rc<Cell<Option<(f64, f64, f64, f64)>>> = Rc::new(Cell::new(None));
    canvas.ref_on("mousedown", {
        let weak = weak.clone();
        let drag = drag.clone();
        move |ev| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(ev) = ev.dyn_ref::<MouseEvent>() else {
                return;
            };
            match ev.button() {
                1 => {
                    let (px, py) = state.pan.get();
                    drag.set(Some((ev.client_x() as f64, ev.client_y() as f64, px, py)));
                    ev.prevent_default();
                },
                0 => {
                    state.eg.event(|pc| {
                        state.set_start(pc, None);
                        state.set_end(pc, None);
                    });
                },
                2 => {
                    state.eg.event(|pc| {
                        state.set_end(pc, None);
                    });
                },
                _ => { },
            }
        }
    });
    canvas.ref_on("contextmenu", |ev| {
        ev.prevent_default();
    });
    canvas.ref_own({
        let weak = weak.clone();
        let drag = drag.clone();
        move |_| {
            EventListener::new(&window(), "mousemove", move |ev| {
                let Some(d) = drag.get() else {
                    return;
                };
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let Some(ev) = ev.dyn_ref::<MouseEvent>() else {
                    return;
                };
                let (mx, my, px, py) = d;
                let nx = px + ev.client_x() as f64 - mx;
                let ny = py + ev.client_y() as f64 - my;
                state.eg.event(|pc| {
                    state.pan.set(pc, (nx, ny));
                });
            })
        }
    });
    canvas.ref_own({
        let drag = drag.clone();
        move |_| {
            EventListener::new(&window(), "mouseup", move |_| {
                drag.set(None);
            })
        }
    });
    canvas.ref_on_with_options("wheel", gloo_events::EventListenerOptions::enable_prevent_default(), {
        let weak = weak.clone();
        move |ev| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(wev) = ev.dyn_ref::<WheelEvent>() else {
                return;
            };
            ev.prevent_default();
            let target = ev.current_target().unwrap().dyn_into::<web_sys::Element>().unwrap();
            let rect = target.get_bounding_client_rect();
            let cx = wev.client_x() as f64 - rect.left();
            let cy = wev.client_y() as f64 - rect.top();
            let factor = (1.1f64).powf(-wev.delta_y() / 100.);
            state.eg.event(|pc| {
                state.cmd_zoom(pc, factor, Some((cx, cy)));
            });
        }
    });
}
