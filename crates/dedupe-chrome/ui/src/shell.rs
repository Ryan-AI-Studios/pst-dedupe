//! Shared matter TopBar + StatusBar (track 0123).

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

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
            <div class="right-slot"></div>
        </header>
    }
}

#[component]
fn StatusBar(flag: &'static str) -> impl IntoView {
    view! {
        <footer class="matter-statusbar">
            <div class="status-left"></div>
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
    }
}
