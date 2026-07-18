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
    /// Placeholder for later tracks.
    StubProduce,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Workspace => "Workspace",
            Self::StubReduce => "Reduce",
            Self::Review => "Review",
            Self::StubProduce => "Produce",
        }
    }

    pub fn is_stub(self) -> bool {
        matches!(self, Self::StubReduce | Self::StubProduce)
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
}
