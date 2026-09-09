use {
    crate::widget::{
        Widget,
        state::State,
    },
    gloo_timers::callback::Timeout,
    gloo_utils::window,
    grafdag_core::Document,
    lunk::EventGraph,
    rooting::set_root,
    std::{
        cell::RefCell,
        rc::Rc,
    },
    wasm_bindgen::{
        JsCast,
        JsValue,
        prelude::wasm_bindgen,
    },
    wasm_bindgen_futures::{
        JsFuture,
        spawn_local,
    },
    web_sys::{
        Request,
        RequestInit,
        Response,
    },
};

async fn fetch_text(method: &str, url: &str, body: Option<String>) -> Result<String, JsValue> {
    let opts = RequestInit::new();
    opts.set_method(method);
    if let Some(body) = body {
        opts.set_body(&JsValue::from_str(&body));
    }
    let request = Request::new_with_str_and_init(url, &opts)?;
    if method == "POST" || method == "PUT" {
        request.headers().set("Content-Type", "application/json")?;
    }
    let resp = JsFuture::from(window().fetch_with_request(&request)).await?;
    let resp: Response = resp.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!("HTTP {}", resp.status())));
    }
    let text = JsFuture::from(resp.text()?).await?;
    return Ok(text.as_string().unwrap_or_default());
}

fn inline_doc() -> Option<Document> {
    let el = gloo_utils::document().get_element_by_id("gd_doc")?;
    let text = el.text_content()?;
    if text.trim().is_empty() || text.contains("<!--GRAFDAG_DOC-->") {
        return None;
    }
    match serde_json::from_str::<Document>(&text) {
        Ok(d) => return Some(d),
        Err(e) => {
            web_sys::console::error_1(&JsValue::from_str(&format!("Failed to parse inline document: {}", e)));
            return None;
        },
    }
}

fn mount(doc: Document) {
    {
        let eg = EventGraph::new();
        let state_slot: Rc<RefCell<Option<Rc<State>>>> = Rc::new(RefCell::new(None));
        let pending: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let timer: Rc<RefCell<Option<Timeout>>> = Rc::new(RefCell::new(None));
        let on_change: Box<dyn Fn(&Document)> = Box::new({
            let state_slot = state_slot.clone();
            let pending = pending.clone();
            let timer = timer.clone();
            move |doc| {
                let json = serde_json::to_string_pretty(doc).unwrap();
                *pending.borrow_mut() = Some(json);
                if let Some(state) = state_slot.borrow().as_ref() {
                    set_status(state, "Unsaved changes");
                }
                let pending = pending.clone();
                let state_slot = state_slot.clone();
                *timer.borrow_mut() = Some(Timeout::new(1500, move || {
                    let Some(json) = pending.borrow_mut().take() else {
                        return;
                    };
                    let state_slot = state_slot.clone();
                    spawn_local(async move {
                        if let Some(state) = state_slot.borrow().as_ref() {
                            set_status(state, "Saving\u{2026}");
                        }
                        let result = fetch_text("POST", "/api/doc", Some(json)).await;
                        if let Some(state) = state_slot.borrow().as_ref() {
                            match result {
                                Ok(_) => set_status(state, "Saved"),
                                Err(e) => {
                                    web_sys::console::error_1(&e);
                                    set_status(state, "Save failed");
                                },
                            }
                        }
                    });
                }));
            }
        });
        let widget = Widget::new(&eg, doc, on_change);
        *state_slot.borrow_mut() = Some(widget.state().clone());
        set_status(widget.state(), "Loaded");
        let root = widget.el().clone();
        set_root(vec![root.clone()]);
        widget.refresh();
        root.ref_own(|_| widget);
    }
}

fn set_status(state: &Rc<State>, text: &str) {
    let text = text.to_string();
    state.eg.event(|pc| {
        state.status.set(pc, text);
    });
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    if let Some(doc) = inline_doc() {
        mount(doc);
        return;
    }
    spawn_local(async {
        let doc = if let Some(d) = inline_doc() {
            d
        } else {
            match fetch_text("GET", "/api/doc", None).await {
                Ok(text) => match serde_json::from_str::<Document>(&text) {
                    Ok(d) => d,
                    Err(e) => {
                        web_sys::console::error_1(&JsValue::from_str(&format!("Failed to parse document: {}", e)));
                        Document::default()
                    },
                },
                Err(e) => {
                    web_sys::console::error_1(&e);
                    Document::default()
                },
            }
        };
        mount(doc);
    });
}
