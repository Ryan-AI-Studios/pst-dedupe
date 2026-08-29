use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::invoke::{tauri_invoke, MatterOverview, RootArgs};
use crate::path_id::encode_matter_id;

#[component]
pub fn MatterHome() -> impl IntoView {
    let params = use_params_map();
    let overview = RwSignal::new(Option::<MatterOverview>::None);
    let error = RwSignal::new(Option::<String>::None);
    let root_sig = RwSignal::new(String::new());
    let navigate = StoredValue::new(use_navigate());

    Effect::new(move |_| {
        // ParamsMap already URL-decodes `:id` — treat as absolute matter root.
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            overview.set(None);
            error.set(Some("Missing matter id in route.".into()));
            return;
        }
        root_sig.set(root.clone());
        error.set(None);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<MatterOverview, _>(
                "matter_overview",
                &RootArgs { root: root.clone() },
            )
            .await
            {
                Ok(ov) => overview.set(Some(ov)),
                Err(e) => {
                    overview.set(None);
                    error.set(Some(e));
                }
            }
        });
    });

    let id_encoded = move || encode_matter_id(&root_sig.get());

    view! {
        <section>
            <div class="toolbar">
                <A href="/matters">"← Matters"</A>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || overview.get().is_some()>
                {move || overview.get().map(|ov| {
                    let custodian_value = if ov.custodians_plus {
                        format!("{}+", ov.custodians)
                    } else {
                        ov.custodians.to_string()
                    };
                    let custodian_tip = if ov.custodians_plus {
                        format!(
                            "Top-N custodian labels shown ({}). The + remainder is {} items not covered by those labels — not extra custodians.",
                            ov.custodians, ov.other_custodians_item_count
                        )
                    } else {
                        "Custodian labels from overview top-N buckets.".into()
                    };
                    let id = id_encoded();
                    view! {
                        <h1>{ov.name.clone()}</h1>
                        <div class="meta-row" id="counts" tabindex="-1">
                            "Schema " {ov.schema_version}
                            " · Generated " <code>{ov.generated_at.clone()}</code>
                            " · Root " <code>{root_sig.get()}</code>
                        </div>
                        <div class="chip-strip">
                            <div class="chip" title="Registered sources">
                                <span class="label">"Sources"</span>
                                <span class="value">{ov.sources}</span>
                            </div>
                            <div
                                class="chip"
                                title="Top-level items only (role IS NULL or role ≠ attachment) — not attachments"
                            >
                                <span class="label">"Processed"</span>
                                <span class="value">{ov.processed}</span>
                            </div>
                            <div class="chip" title="Matter-scoped item_errors">
                                <span class="label">"Exceptions"</span>
                                <span class="value">{ov.exceptions}</span>
                            </div>
                            <div class="chip" title="In-review items with zero codes">
                                <span class="label">"Unreviewed"</span>
                                <span class="value">{ov.unreviewed}</span>
                            </div>
                            <div class="chip privilege" title="Active privilege claims (not withhold)">
                                <span class="label">"Privileged"</span>
                                <span class="value">{ov.privileged}</span>
                            </div>
                            <div class="chip withhold" title="Privilege withhold flag / table union">
                                <span class="label">"Withhold"</span>
                                <span class="value">{ov.withhold}</span>
                            </div>
                            <div class="chip" title=custodian_tip.clone()>
                                <span class="label">"Custodians"</span>
                                <span class="value">{custodian_value}</span>
                            </div>
                            <div class="chip" title="Produce checklist ships in track 0113">
                                <span class="label">"Produced"</span>
                                <span class="value">"—"</span>
                                <span class="label">"0113"</span>
                            </div>
                        </div>
                        <div class="cta-row">
                            <button
                                class="primary"
                                on:click={
                                    let id = id.clone();
                                    move |_| navigate.with_value(|nav| {
                                        nav(&format!("/matters/{id}/process"), Default::default())
                                    })
                                }
                            >
                                "Ingest / Process"
                            </button>
                            <button on:click={
                                let id = id.clone();
                                move |_| navigate.with_value(|nav| {
                                    nav(&format!("/matters/{id}/review"), Default::default())
                                })
                            }>"Continue review"</button>
                            <button on:click={
                                let id = id.clone();
                                move |_| navigate.with_value(|nav| {
                                    nav(&format!("/matters/{id}/produce"), Default::default())
                                })
                            }>"Produce"</button>
                        </div>
                        <nav class="tabs" aria-label="Matter workspace">
                            <A href=format!("/matters/{id}")>"Home"</A>
                            <A href=format!("/matters/{id}/process")>"Process"</A>
                            <A href=format!("/matters/{id}/review")>"Review"</A>
                            <A href=format!("/matters/{id}/produce")>"Produce"</A>
                            <A href=format!("/matters/{id}/admin")>"Admin"</A>
                        </nav>
                        <p class="empty">"Matter home — overview chips above match Desk load_case_overview rollups."</p>
                    }
                })}
            </Show>
        </section>
    }
}
