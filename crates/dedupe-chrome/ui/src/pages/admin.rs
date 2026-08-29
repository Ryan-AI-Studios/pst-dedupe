use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::path_id::matter_home_href_from_param;

#[component]
pub fn AdminStub() -> impl IntoView {
    let params = use_params_map();
    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));
    view! {
        <section>
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
            </div>
            <h1>"Admin"</h1>
            <div class="stub-panel">
                <p>"Admin is inert in this track."</p>
            </div>
        </section>
    }
}
