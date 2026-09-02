//! Shared matter TopBar + StatusBar (track 0123).

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use web_sys::KeyboardEvent;

use crate::invoke::{tauri_invoke, MatterOverview, RootArgs};
use crate::path_id::encode_matter_id;

pub const PROCESS_FLAG: &str =
    "Processing is deterministic. No prediction, no coding, no privilege calls here.";
pub const REVIEW_FLAG: &str = "Privilege column is coding (PRIV), not withhold";
pub const PRODUCE_FLAG: &str =
    "A privileged document cannot enter a production set without a documented override";
pub const ADMIN_FLAG: &str = "Admin is a later design batch.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTab {
    Home,
    Process,
    Review,
    Produce,
    Admin,
}

impl WorkspaceTab {
    fn flag(self) -> &'static str {
        match self {
            WorkspaceTab::Home => "",
            WorkspaceTab::Process => PROCESS_FLAG,
            WorkspaceTab::Review => REVIEW_FLAG,
            WorkspaceTab::Produce => PRODUCE_FLAG,
            WorkspaceTab::Admin => ADMIN_FLAG,
        }
    }
}

#[derive(Clone, Copy)]
pub struct MatterShellCtx {
    pub root: RwSignal<String>,
    pub overview: RwSignal<Option<MatterOverview>>,
    pub error: RwSignal<Option<String>>,
}

/// Queue-route chrome for the reserved TopBar right slot and StatusBar left.
/// Provided by `WrapReview` only — not the review window route.
#[derive(Clone, Copy)]
pub struct QueueChromeCtx {
    pub queue_range: RwSignal<Option<QueueRange>>,
    pub goto_request: RwSignal<Option<String>>,
    pub goto_miss: RwSignal<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueRange {
    pub offset: u64,
    pub fetched: usize,
    pub total: u64,
}

impl QueueRange {
    pub fn status_label(self) -> String {
        if self.fetched > 0 {
            let start = self.offset.saturating_add(1);
            let end = self.offset.saturating_add(self.fetched as u64);
            format!("Rows {start}–{end} of {}", self.total)
        } else if self.total > 0 {
            "This page has no rows, but the queue still has items. Use Prev/Next.".into()
        } else {
            "0 in queue".into()
        }
    }
}

pub fn fallback_matter_name(root: &str) -> String {
    let trimmed = root.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Matter")
        .to_string()
}

#[component]
pub fn MatterShell(tab: WorkspaceTab, children: Children) -> impl IntoView {
    let params = use_params_map();
    let overview = RwSignal::new(Option::<MatterOverview>::None);
    let error = RwSignal::new(Option::<String>::None);
    let root_sig = RwSignal::new(String::new());

    provide_context(MatterShellCtx {
        root: root_sig,
        overview,
        error,
    });

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            overview.set(None);
            error.set(Some("Missing matter id in route.".into()));
            root_sig.set(String::new());
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
                Ok(ov) => {
                    if root_sig.get_untracked() == root {
                        overview.set(Some(ov));
                        error.set(None);
                    }
                }
                Err(e) => {
                    if root_sig.get_untracked() == root {
                        overview.set(None);
                        error.set(Some(e));
                    }
                }
            }
        });
    });

    view! {
        <div class="matter-shell">
            <TopBar tab=tab root=root_sig overview=overview />
            <div class="matter-shell-body">{children()}</div>
            <StatusBar flag=tab.flag() />
        </div>
    }
}

#[component]
fn TopBar(
    tab: WorkspaceTab,
    root: RwSignal<String>,
    overview: RwSignal<Option<MatterOverview>>,
) -> impl IntoView {
    let encoded = move || encode_matter_id(&root.get());
    let display_name = move || {
        overview
            .get()
            .map(|o| o.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback_matter_name(&root.get()))
    };
    let processed_meta = move || overview.get().map(|o| format!("Processed {}", o.processed));
    let queue_chrome = use_context::<QueueChromeCtx>();
    let goto_draft = RwSignal::new(String::new());

    view! {
        <header class="matter-topbar">
            <a class="brand" href=move || format!("/matters/{}", encoded())>
                "Dedupe Desk"
            </a>
            <A
                href=move || format!("/matters/{}", encoded())
                exact=true
                {..}
                class="matter-name"
                attr:aria-current=move || (tab == WorkspaceTab::Home).then_some("page")
            >
                {display_name}
            </A>
            <Show when=move || processed_meta().is_some()>
                <span class="meta">{move || processed_meta().unwrap_or_default()}</span>
            </Show>
            <A href="/matters" exact=true {..} class="leave-matter">
                "← Matters"
            </A>
            <nav class="workspace-tabs" aria-label="Matter workspace">
                <A
                    href=move || format!("/matters/{}/process", encoded())
                    exact=true
                    {..}
                    class="workspace-tab"
                    class:active=tab == WorkspaceTab::Process
                >
                    "Process"
                </A>
                <A
                    href=move || format!("/matters/{}/review", encoded())
                    {..}
                    class="workspace-tab"
                    class:active=tab == WorkspaceTab::Review
                >
                    "Review"
                </A>
                <A
                    href=move || format!("/matters/{}/produce", encoded())
                    exact=true
                    {..}
                    class="workspace-tab"
                    class:active=tab == WorkspaceTab::Produce
                >
                    "Produce"
                </A>
                <span class="workspace-tab-inert" aria-disabled="true">
                    "Admin"
                </span>
            </nav>
            {match queue_chrome {
                Some(ctx) => {
                    view! {
                        <div class="right-slot">
                            <input
                                id="queue-goto"
                                type="search"
                                placeholder="Go to Control# or subject"
                                aria-label="Go to Control# or subject"
                                prop:value=move || goto_draft.get()
                                on:input=move |ev| {
                                    goto_draft.set(event_target_value(&ev));
                                    ctx.goto_miss.set(None);
                                }
                                on:keydown=move |ev: KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        ctx.goto_request.set(Some(goto_draft.get()));
                                    }
                                }
                            />
                            <Show when=move || ctx.goto_miss.get().is_some()>
                                <span class="queue-goto-miss" role="status">
                                    {move || ctx.goto_miss.get().unwrap_or_default()}
                                </span>
                            </Show>
                        </div>
                    }
                    .into_any()
                }
                None => view! { <div class="right-slot"></div> }.into_any(),
            }}
        </header>
    }
}

#[component]
fn StatusBar(flag: &'static str) -> impl IntoView {
    let queue_chrome = use_context::<QueueChromeCtx>();
    view! {
        <footer class="matter-statusbar">
            {match queue_chrome {
                Some(ctx) => view! {
                    <div class="status-left">
                        {move || {
                            ctx.queue_range
                                .get()
                                .map(QueueRange::status_label)
                                .unwrap_or_default()
                        }}
                    </div>
                }
                .into_any(),
                None => view! { <div class="status-left"></div> }.into_any(),
            }}
            <div class="flag">{flag}</div>
        </footer>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_name_uses_last_windows_segment() {
        assert_eq!(fallback_matter_name(r"C:\Cases\Smith"), "Smith");
        assert_eq!(fallback_matter_name(r"C:\Cases\Smith\"), "Smith");
        assert_eq!(fallback_matter_name("/tmp/demo"), "demo");
        assert_eq!(fallback_matter_name(""), "Matter");
        assert_eq!(fallback_matter_name("\\"), "Matter");
    }

    #[test]
    fn shell_source_locks() {
        let src = include_str!("shell.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains(PROCESS_FLAG));
        assert!(prod.contains("workspace-tab-inert"));
        assert!(prod.contains("Dedupe Desk"));
        assert!(!prod.contains("Archivo"));
        assert!(!prod.contains("#ec3013"));
        assert!(!prod.contains("DEDUPE / REVIEW"));
        assert!(!prod.contains("href=\"/process\""));
        assert!(prod.contains("← Matters"));
        assert!(
            !prod.contains("href=\"/matters/{}/admin\""),
            "Admin must not be a workspace tab link"
        );
        assert!(prod.contains("<span class=\"workspace-tab-inert\""));
        assert!(prod.contains("id=\"queue-goto\""));
        assert!(prod.contains("class=\"right-slot\""));
        assert!(prod.contains("class=\"status-left\""));
        assert!(prod.contains(REVIEW_FLAG));
    }

    #[test]
    fn queue_range_status_label_is_sql_page() {
        assert_eq!(
            QueueRange {
                offset: 0,
                fetched: 500,
                total: 1200
            }
            .status_label(),
            "Rows 1–500 of 1200"
        );
        assert_eq!(
            QueueRange {
                offset: 500,
                fetched: 200,
                total: 700
            }
            .status_label(),
            "Rows 501–700 of 700"
        );
        assert_eq!(
            QueueRange {
                offset: 500,
                fetched: 0,
                total: 400
            }
            .status_label(),
            "This page has no rows, but the queue still has items. Use Prev/Next."
        );
        assert_eq!(
            QueueRange {
                offset: 0,
                fetched: 0,
                total: 0
            }
            .status_label(),
            "0 in queue"
        );
    }
}
