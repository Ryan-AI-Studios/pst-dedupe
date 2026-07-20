//! Navigation state machine (pure helpers — unit tested).

/// Top-level desk screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Create / open / recent matters.
    Home,
    /// Sources, process, jobs, stats for the open matter.
    Workspace,
    /// Placeholder for later tracks.
    StubReduce,
    /// Review corpus list + body viewer (track 0026).
    Review,
    /// Production export (track 0040) — produce dialog + job.
    Produce,
    /// Gap analysis: expected custodians + opposing DAT (track 0042).
    Gap,
    /// People–comms graph tables (track 0047).
    People,
    /// Concept / theme clusters (track 0048).
    Clusters,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Workspace => "Workspace",
            Self::StubReduce => "Reduce",
            Self::Review => "Review",
            Self::Produce => "Produce",
            Self::Gap => "Gap",
            Self::People => "People",
            Self::Clusters => "Clusters",
        }
    }

    pub fn is_stub(self) -> bool {
        // Produce + Gap are live; Reduce remains a placeholder.
        matches!(self, Self::StubReduce)
    }

    /// Whether this screen requires an open matter root.
    pub fn requires_matter(self) -> bool {
        !matches!(self, Self::Home)
    }
}

/// Resolve navigation intent when the operator clicks a nav item.
///
/// Returns the target screen, or `None` if the click is ignored (e.g. Workspace
/// without a matter).
pub fn resolve_nav(current: Screen, target: Screen, has_matter: bool) -> Screen {
    if target == Screen::Home {
        return Screen::Home;
    }
    if target.requires_matter() && !has_matter {
        return current;
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_always_reachable() {
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::Home, true),
            Screen::Home
        );
    }

    #[test]
    fn workspace_blocked_without_matter() {
        assert_eq!(
            resolve_nav(Screen::Home, Screen::Workspace, false),
            Screen::Home
        );
        assert_eq!(
            resolve_nav(Screen::Home, Screen::Workspace, true),
            Screen::Workspace
        );
    }

    #[test]
    fn review_needs_matter_and_is_not_stub() {
        assert!(!Screen::Review.is_stub());
        assert_eq!(Screen::Review.label(), "Review");
        assert_eq!(
            resolve_nav(Screen::Home, Screen::Review, false),
            Screen::Home
        );
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::Review, true),
            Screen::Review
        );
    }

    #[test]
    fn reduce_still_stub() {
        assert!(Screen::StubReduce.is_stub());
    }

    #[test]
    fn produce_is_live_not_stub() {
        assert!(!Screen::Produce.is_stub());
        assert_eq!(Screen::Produce.label(), "Produce");
        assert_eq!(
            resolve_nav(Screen::Home, Screen::Produce, false),
            Screen::Home
        );
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::Produce, true),
            Screen::Produce
        );
    }

    #[test]
    fn gap_is_live_not_stub() {
        assert!(!Screen::Gap.is_stub());
        assert_eq!(Screen::Gap.label(), "Gap");
        assert_eq!(resolve_nav(Screen::Home, Screen::Gap, false), Screen::Home);
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::Gap, true),
            Screen::Gap
        );
    }

    #[test]
    fn people_is_live_not_stub() {
        assert!(!Screen::People.is_stub());
        assert_eq!(Screen::People.label(), "People");
        assert_eq!(
            resolve_nav(Screen::Home, Screen::People, false),
            Screen::Home
        );
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::People, true),
            Screen::People
        );
    }

    #[test]
    fn clusters_is_live_when_matter_open() {
        assert!(!Screen::Clusters.is_stub());
        assert_eq!(Screen::Clusters.label(), "Clusters");
        assert_eq!(
            resolve_nav(Screen::Home, Screen::Clusters, false),
            Screen::Home
        );
        assert_eq!(
            resolve_nav(Screen::Workspace, Screen::Clusters, true),
            Screen::Clusters
        );
    }
}
