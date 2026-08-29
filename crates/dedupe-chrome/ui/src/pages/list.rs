use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use wasm_bindgen::prelude::*;

use crate::invoke::{tauri_invoke, CreateArgs, RecentMatter, RememberArgs};
use crate::path_id::encode_matter_id;

#[component]
pub fn MattersList() -> impl IntoView {
    let recents = RwSignal::new(Vec::<RecentMatter>::new());
    let error = RwSignal::new(Option::<String>::None);
    let search = RwSignal::new(String::new());
    let show_create = RwSignal::new(false);
    let new_name = RwSignal::new(String::new());
    let new_parent = RwSignal::new(String::new());
    let navigate = StoredValue::new(use_navigate());

    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            match tauri_invoke::<Vec<RecentMatter>, serde_json::Value>(
                "recent_matters_list",
                &serde_json::json!({}),
            )
            .await
            {
                Ok(list) => recents.set(list),
                Err(e) => error.set(Some(e)),
            }
        });
    });

    let go_matter = move |root: String, name: String| {
        leptos::task::spawn_local(async move {
            match tauri_invoke::<Vec<RecentMatter>, _>(
                "recent_matters_remember",
                &RememberArgs {
                    root: root.clone(),
                    name,
                },
            )
            .await
            {
                Ok(list) => {
                    recents.set(list);
                    error.set(None);
                    clear_chrome_status();
                }
                // Best-effort persist: show shell status (survives navigate), still open matter.
                Err(e) => {
                    let msg = format!("Could not update recents: {e}");
                    error.set(Some(msg.clone()));
                    set_chrome_status(&msg);
                }
            }
            let id = encode_matter_id(&root);
            navigate.with_value(|nav| nav(&format!("/matters/{id}"), Default::default()));
        });
    };

    view! {
        <section id="matters" tabindex="-1">
            <h1>"Matters"</h1>
            <div class="toolbar">
                <input
                    id="matter-search"
                    type="search"
                    placeholder="Search recents (Ctrl+K)"
                    prop:value=move || search.get()
                    on:input=move |ev| search.set(event_target_value(&ev))
                />
                <button
                    class="primary"
                    on:click=move |_| {
                        leptos::task::spawn_local(async move {
                            match pick_folder().await {
                                Ok(Some(path)) => {
                                    new_parent.set(path);
                                    show_create.set(true);
                                }
                                Ok(None) => {}
                                Err(e) => error.set(Some(e)),
                            }
                        });
                    }
                >
                    "New matter…"
                </button>
                <button on:click=move |_| {
                    leptos::task::spawn_local(async move {
                        match pick_folder().await {
                            Ok(Some(path)) => {
                                let name = path
                                    .rsplit(['\\', '/'])
                                    .next()
                                    .unwrap_or(path.as_str())
                                    .to_string();
                                go_matter(path, name);
                            }
                            Ok(None) => {}
                            Err(e) => error.set(Some(e)),
                        }
                    });
                }>"Open…"</button>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || show_create.get()>
                <div class="dialog-form">
                    <label>
                        "Parent folder"
                        <input
                            type="text"
                            prop:value=move || new_parent.get()
                            on:input=move |ev| new_parent.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "Matter name"
                        <input
                            type="text"
                            prop:value=move || new_name.get()
                            on:input=move |ev| new_name.set(event_target_value(&ev))
                        />
                    </label>
                    <div class="toolbar">
                        <button
                            class="primary"
                            on:click=move |_| {
                                let parent = new_parent.get();
                                // Host trims for Matter::create; keep recents display in sync.
                                let name = new_name.get().trim().to_string();
                                leptos::task::spawn_local(async move {
                                    match tauri_invoke::<String, _>(
                                        "create_matter",
                                        &CreateArgs {
                                            parent,
                                            name: name.clone(),
                                        },
                                    )
                                    .await
                                    {
                                        Ok(root) => {
                                            show_create.set(false);
                                            go_matter(root, name);
                                        }
                                        Err(e) => error.set(Some(e)),
                                    }
                                });
                            }
                        >
                            "Create"
                        </button>
                        <button on:click=move |_| show_create.set(false)>"Cancel"</button>
                    </div>
                </div>
            </Show>
            <Show
                when=move || !recents.get().is_empty()
                fallback=|| view! { <p class="empty">"No recent matters yet. Create or Open a matter folder."</p> }
            >
                <div class="card-grid">
                    <For
                        each=move || {
                            let q = search.get().trim().to_lowercase();
                            recents
                                .get()
                                .into_iter()
                                .filter(|m| {
                                    q.is_empty()
                                        || m.name.to_lowercase().contains(&q)
                                        || m.root.to_lowercase().contains(&q)
                                })
                                .collect::<Vec<_>>()
                        }
                        key=|m| m.root.clone()
                        children=move |m| {
                            let root = m.root.clone();
                            let name = m.name.clone();
                            view! {
                                <button
                                    class="matter-card"
                                    on:click=move |_| go_matter(root.clone(), name.clone())
                                >
                                    <div class="name">{m.name.clone()}</div>
                                    <div class="path">{m.root.clone()}</div>
                                </button>
                            }
                        }
                    />
                </div>
            </Show>
        </section>
    }
}

fn set_chrome_status(msg: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = doc.get_element_by_id("chrome-status") {
            el.set_text_content(Some(msg));
            let _ = el.set_attribute("data-visible", "true");
        }
    }
}

fn clear_chrome_status() {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = doc.get_element_by_id("chrome-status") {
            el.set_text_content(Some(""));
            let _ = el.remove_attribute("data-visible");
        }
    }
}

async fn pick_folder() -> Result<Option<String>, String> {
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"directory".into(), &JsValue::TRUE)
        .map_err(|e| format!("{e:?}"))?;
    js_sys::Reflect::set(&opts, &"multiple".into(), &JsValue::FALSE)
        .map_err(|e| format!("{e:?}"))?;
    let promise = dialog_open(opts.into()).map_err(|e| format!("{e:?}"))?;
    let value = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| format!("{e:?}"))?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    if let Some(s) = value.as_string() {
        return Ok(Some(s));
    }
    let arr = js_sys::Array::from(&value);
    if let Some(first) = arr.get(0).as_string() {
        return Ok(Some(first));
    }
    Ok(None)
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open, catch)]
    fn dialog_open(options: JsValue) -> Result<js_sys::Promise, JsValue>;
}
