//! Keyboard and mouse handling.
use {
    super::state::{
        Mode,
        SearchTarget,
        State,
        Vec2,
    },
    grafdag_core::layout::ScreenDir,
    gloo_events::{
        EventListener,
        EventListenerOptions,
    },
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

/// Pixels the mouse must move before a left press on the canvas becomes a pan
/// rather than a click.
const DRAG_THRESHOLD: f64 = 3.;

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
            // Arrows are relative to the layout's flow direction
            "ArrowLeft" => state.cmd_arrow(pc, ScreenDir::Left, shift),
            "ArrowRight" => state.cmd_arrow(pc, ScreenDir::Right, shift),
            "ArrowDown" => state.cmd_arrow(pc, ScreenDir::Down, shift),
            "ArrowUp" => state.cmd_arrow(pc, ScreenDir::Up, shift),
            "PageDown" => state.cmd_island(pc, true),
            "PageUp" => state.cmd_island(pc, false),
            "Tab" => state.cmd_toggle_panel(pc),
            "Escape" => state.cmd_escape(pc),
            "l" => state.cmd_link(pc),
            "u" => state.cmd_unlink(pc),
            "r" => state.cmd_reverse(pc),
            "n" => state.cmd_new_next(pc),
            "s" => state.cmd_new_sibling(pc),
            "N" => state.cmd_new_island(pc),
            "Delete" | "Backspace" => state.cmd_delete(pc),
            "Enter" => state.cmd_enter(pc),
            "e" => state.cmd_edit(pc),
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
        EventListener::new_with_options(&document(), "keydown", EventListenerOptions::enable_prevent_default(), move |ev| {
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

/// Attach mouse handling to the canvas: left drag on empty space pans, wheel
/// zooms, left/right clicks (without dragging) on empty space clear the
/// selection.
pub fn attach_canvas(canvas: &El, state: &Rc<State>) {
    let weak: Weak<State> = Rc::downgrade(state);
    // Pan drag state: (start mouse x, y, start pan x, y, moved past threshold)
    let drag: Rc<Cell<Option<(f64, f64, f64, f64, bool)>>> = Rc::new(Cell::new(None));
    canvas.ref_on_with_options("mousedown", EventListenerOptions::enable_prevent_default(), {
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
                0 => {
                    let Vec2(px, py) = state.pan.get();
                    drag.set(Some((ev.client_x() as f64, ev.client_y() as f64, px, py, false)));
                    ev.prevent_default();
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
    canvas.ref_on_with_options("contextmenu", EventListenerOptions::enable_prevent_default(), |ev| {
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
                let (mx, my, px, py, moved) = d;
                let dx = ev.client_x() as f64 - mx;
                let dy = ev.client_y() as f64 - my;
                // Small jitter during a click shouldn't turn it into a drag
                let moved = moved || dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD;
                if !moved {
                    return;
                }
                drag.set(Some((mx, my, px, py, true)));
                state.eg.event(|pc| {
                    state.set_pan(pc, Vec2(px + dx, py + dy));
                });
            })
        }
    });
    canvas.ref_own({
        let weak = weak.clone();
        let drag = drag.clone();
        move |_| {
            EventListener::new(&window(), "mouseup", move |ev| {
                let Some(d) = drag.take() else {
                    return;
                };
                let Some(ev) = ev.dyn_ref::<MouseEvent>() else {
                    return;
                };
                if ev.button() != 0 || d.4 {
                    return;
                }
                // Left click without dragging on empty space clears the selection
                let Some(state) = weak.upgrade() else {
                    return;
                };
                state.eg.event(|pc| {
                    state.set_start(pc, None);
                    state.set_end(pc, None);
                });
            })
        }
    });
    canvas.ref_on_with_options("wheel", EventListenerOptions::enable_prevent_default(), {
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
