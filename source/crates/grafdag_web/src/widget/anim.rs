use {
    gloo_render::{
        AnimationFrame,
        request_animation_frame,
    },
    gloo_utils::window,
    lunk::{
        Animator,
        EventGraph,
    },
    std::{
        cell::{
            Cell,
            RefCell,
        },
        rc::Rc,
    },
};

pub const TRANSITION_MS: f64 = 250.;

pub fn ease(t: f64) -> f64 {
    return ezing::quad_out(t);
}

pub fn new_animator(eg: &EventGraph) -> Animator {
    let anim = Animator::new();
    let frame: Rc<RefCell<Option<AnimationFrame>>> = Rc::new(RefCell::new(None));
    let last_ts: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));

    fn next_frame(
        frame: Rc<RefCell<Option<AnimationFrame>>>,
        last_ts: Rc<Cell<Option<f64>>>,
        eg: EventGraph,
        anim: Animator,
    ) {
        let handle = request_animation_frame({
            let frame = frame.clone();
            let last_ts = last_ts.clone();
            move |ts| {
                let delta = last_ts.get().map(|l| (ts - l).clamp(0., 100.)).unwrap_or(16.);
                last_ts.set(Some(ts));
                if anim.update(&eg, delta) {
                    next_frame(frame, last_ts, eg, anim);
                } else {
                    *frame.borrow_mut() = None;
                    last_ts.set(None);
                }
            }
        });
        *frame.borrow_mut() = Some(handle);
    }

    anim.set_start_cb({
        let anim = anim.clone();
        let eg = eg.clone();
        move || {
            if frame.borrow().is_some() {
                return;
            }
            next_frame(frame.clone(), last_ts.clone(), eg.clone(), anim.clone());
        }
    });
    return anim;
}

pub fn prefers_reduced_motion() -> bool {
    return window()
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);
}
