use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::path_id::matter_home_href_from_param;

#[component]
pub fn ProcessStub() -> impl IntoView {
    let params = use_params_map();
    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));
    view! {
        <section>
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
            </div>
            <h1>"Process"</h1>
            <div class="stub-panel">
                <p>"Process stays in Dedupe Desk until 0116."</p>
            </div>
        </section>
    }
}
