use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::path_id::{encode_matter_id, matter_home_href_from_param};

#[component]
pub fn ReviewDocStub() -> impl IntoView {
    let params = use_params_map();
    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));
    let review = move || {
        params.with(|p| {
            let id = p.get("id").unwrap_or_default();
            format!("/matters/{}/review", encode_matter_id(&id))
        })
    };
    let doc_id = move || params.with(|p| p.get("docId").unwrap_or_default());
    view! {
        <section>
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
                <A href=review>"← Queue"</A>
            </div>
            <h1>"Review window"</h1>
            <div class="stub-panel">
                <p>"Review window is 0112."</p>
                <p class="empty">"Document: " <code>{doc_id}</code></p>
            </div>
        </section>
    }
}
