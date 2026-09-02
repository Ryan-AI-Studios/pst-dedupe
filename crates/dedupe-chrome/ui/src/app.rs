use std::sync::Once;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::MouseEvent;

use crate::pages::{
    AdminStub, MatterHome, MattersList, ProcessPage, ProducePage, ReviewQueue, ReviewWindow,
};
use crate::shell::{MatterShell, QueueChromeCtx, WorkspaceTab};

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
            if let Some(el) = doc.get_element_by_id("queue-goto") {
                if let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() {
                    ev.prevent_default();
                    let _ = input.focus();
                    if let Some(hint) = doc.get_element_by_id("ctrl-k-hint") {
                        let _ = hint.remove_attribute("data-visible");
                    }
                    return;
                }
            }
            // Search / Go-to not mounted (matter home / stubs / review window): visible no-op.
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
fn MattersLauncher() -> impl IntoView {
    view! {
        <div class="matters-launcher">
            <header class="top-bar launcher-top-bar">
                <div class="brand">"Dedupe Desk"</div>
                <div class="hint">"Matter chrome · overview from matter-core"</div>
            </header>
            <MattersList/>
        </div>
    }
}

fn wrap_home() -> impl IntoView {
    view! {
        <MatterShell tab=WorkspaceTab::Home>
            <MatterHome/>
        </MatterShell>
    }
}

fn wrap_process() -> impl IntoView {
    view! {
        <MatterShell tab=WorkspaceTab::Process>
            <ProcessPage/>
        </MatterShell>
    }
}

#[component]
fn WrapReview() -> impl IntoView {
    provide_context(QueueChromeCtx {
        queue_range: RwSignal::new(None),
        goto_request: RwSignal::new(None),
        goto_miss: RwSignal::new(None),
    });
    view! {
        <MatterShell tab=WorkspaceTab::Review>
            <ReviewQueue/>
        </MatterShell>
    }
}

fn wrap_review() -> impl IntoView {
    view! { <WrapReview/> }
}

fn wrap_review_window() -> impl IntoView {
    view! {
        <MatterShell tab=WorkspaceTab::Review>
            <ReviewWindow/>
        </MatterShell>
    }
}

fn wrap_produce() -> impl IntoView {
    view! {
        <MatterShell tab=WorkspaceTab::Produce>
            <ProducePage/>
        </MatterShell>
    }
}

fn wrap_admin() -> impl IntoView {
    view! {
        <MatterShell tab=WorkspaceTab::Admin>
            <AdminStub/>
        </MatterShell>
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
                <a
                    href="#queue"
                    on:click=move |ev: MouseEvent| {
                        ev.prevent_default();
                        focus_skip_target("queue");
                    }
                >
                    "Skip to queue"
                </a>
                <a
                    href="#document"
                    on:click=move |ev: MouseEvent| {
                        ev.prevent_default();
                        focus_skip_target("document");
                    }
                >
                    "Skip to document"
                </a>
            </div>
            <div id="ctrl-k-hint" class="chord-hint app-chord-hint" role="status" aria-live="polite">
                "Ctrl+K focuses matter search on the Matters list, or Go-to on the review queue."
            </div>
            <div
                id="chrome-status"
                class="chrome-status"
                role="status"
                aria-live="polite"
            ></div>
            <main id="main-content" class="main-surface" tabindex="-1">
                <Router>
                    <Routes fallback=|| {
                        view! { <p class="empty route-fallback">"Not found"</p> }
                    }>
                        <Route path=path!("/") view=|| {
                            view! {
                                <leptos_router::components::Redirect path="/matters"/>
                            }
                        } />
                        <Route path=path!("/matters") view=MattersLauncher />
                        <Route path=path!("/matters/:id") view=wrap_home />
                        <Route path=path!("/matters/:id/process") view=wrap_process />
                        <Route path=path!("/matters/:id/review") view=wrap_review />
                        <Route path=path!("/matters/:id/review/:docId") view=wrap_review_window />
                        <Route path=path!("/matters/:id/produce") view=wrap_produce />
                        <Route path=path!("/matters/:id/admin") view=wrap_admin />
                    </Routes>
                </Router>
            </main>
        </div>
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ctrl_k_prefers_matter_search_then_queue_goto() {
        let src = include_str!("app.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let search_at = prod
            .find("get_element_by_id(\"matter-search\")")
            .expect("matter-search");
        let goto_at = prod
            .find("get_element_by_id(\"queue-goto\")")
            .expect("queue-goto");
        assert!(search_at < goto_at, "matter-search must win when mounted");
        let window_fn = prod
            .split("fn wrap_review_window()")
            .nth(1)
            .expect("wrap_review_window");
        let window_fn = window_fn
            .split("fn wrap_produce()")
            .next()
            .unwrap_or(window_fn);
        assert!(
            !window_fn.contains("QueueChromeCtx"),
            "review window must not provide queue chrome"
        );
        assert!(prod.contains("provide_context(QueueChromeCtx"));
    }
}
