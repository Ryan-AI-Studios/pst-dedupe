use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::path_id::encode_matter_id;
use crate::shell::MatterShellCtx;

#[component]
pub fn MatterHome() -> impl IntoView {
    let ctx = expect_context::<MatterShellCtx>();
    let overview = ctx.overview;
    let error = ctx.error;
    let root_sig = ctx.root;
    let navigate = StoredValue::new(use_navigate());

    let id_encoded = move || encode_matter_id(&root_sig.get());

    view! {
        <section>
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
                            <div class="chip" title="Distinct items in complete production volumes">
                                <span class="label">"Produced"</span>
                                <span class="value">{ov.produced}</span>
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
                    }
                })}
            </Show>
        </section>
    }
}
