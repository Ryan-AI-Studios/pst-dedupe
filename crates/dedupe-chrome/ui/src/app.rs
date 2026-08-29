use std::sync::Once;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::MouseEvent;

use crate::pages::{AdminStub, MatterHome, MattersList, ProcessStub, ProduceStub, ReviewStub};

static CTRL_K_ONCE: Once = Once::new();

fn show_ctrl_k_hint(doc: &web_sys::Document) {
    let Some(el) = doc.get_element_by_id("ctrl-k-hint") else {
        return;
    };
    let _ = el.set_attribute("data-visible", "true");
    let Some(window) = web_sys::window() else {
        return;
    };
    let el_hide = el.clone();
    let cb = Closure::once(Box::new(move || {
        let _ = el_hide.remove_attribute("data-visible");
    }) as Box<dyn FnOnce()>);
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), 2500);
    cb.forget();
}

fn install_ctrl_k_once() {
    CTRL_K_ONCE.call_once(|| {
        let handler = Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
            if !(ev.ctrl_key() && ev.key().eq_ignore_ascii_case("k")) {
                return;
            }
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                return;
            };
            if let Some(el) = doc.get_element_by_id("matter-search") {
                if let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() {
                    // Swallow only when search can take focus.
                    ev.prevent_default();
                    let _ = input.focus();
                    if let Some(hint) = doc.get_element_by_id("ctrl-k-hint") {
                        let _ = hint.remove_attribute("data-visible");
                    }
                    return;
                }
            }
            // Search not mounted (matter home / stubs): visible no-op per spec §3.2.
            ev.prevent_default();
            show_ctrl_k_hint(&doc);
        }) as Box<dyn FnMut(_)>);
        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        // App-shell lifetime: register once so route remounts do not stack handlers.
        handler.forget();
    });
}

fn focus_skip_target(preferred_id: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let try_focus = |id: &str| -> bool {
        let Some(el) = doc.get_element_by_id(id) else {
            return false;
        };
        let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() else {
            return false;
        };
        html.focus().is_ok()
    };
    if !try_focus(preferred_id) {
        let _ = try_focus("main-content");
    }
}

#[component]
pub fn App() -> impl IntoView {
    Effect::new(move |_| {
        install_ctrl_k_once();
    });

    view! {
        <div class="app-shell">
            <div class="skip-links">
                <a
                    href="#matters"
                    on:click=move |ev: MouseEvent| {
                        ev.prevent_default();
                        focus_skip_target("matters");
                    }
                >
                    "Skip to matters"
                </a>
                <a
                    href="#counts"
                    on:click=move |ev: MouseEvent| {
                        ev.prevent_default();
                        focus_skip_target("counts");
                    }
                >
                    "Skip to counts"
                </a>
            </div>
            <header class="top-bar">
                <div class="brand">"Dedupe Desk"</div>
                <div class="hint">"Matter chrome · overview from matter-core"</div>
                <div id="ctrl-k-hint" class="chord-hint" role="status" aria-live="polite">
                    "Ctrl+K focuses matter search on the Matters list."
                </div>
            </header>
            <div
                id="chrome-status"
                class="chrome-status"
                role="status"
                aria-live="polite"
            ></div>
            <main id="main-content" class="main-surface" tabindex="-1">
                <Router>
                    <Routes fallback=|| {
                        view! { <p class="empty" style="padding: 8px;">"Not found"</p> }
                    }>
                        <Route path=path!("/") view=|| {
                            view! {
                                <leptos_router::components::Redirect path="/matters"/>
                            }
                        } />
                        <Route path=path!("/matters") view=MattersList />
                        <Route path=path!("/matters/:id") view=MatterHome />
                        <Route path=path!("/matters/:id/process") view=ProcessStub />
                        <Route path=path!("/matters/:id/review") view=ReviewStub />
                        <Route path=path!("/matters/:id/produce") view=ProduceStub />
                        <Route path=path!("/matters/:id/admin") view=AdminStub />
                    </Routes>
                </Router>
            </main>
        </div>
    }
}
