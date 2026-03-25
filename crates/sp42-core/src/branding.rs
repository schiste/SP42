//! Centralized branding constants so renaming stays a one-file change.

pub const PROJECT_NAME: &str = "SP42";
pub const PROJECT_SLUG: &str = "sp42";
pub const CONFIG_PAGE_PREFIX: &str = "Project:SP42";
pub const OAUTH_APP_NAME: &str = "SP42";
pub const USER_AGENT: &str = "SP42/0.1.0 (+https://github.com/christophehenner/SP42)";

#[cfg(test)]
mod tests {
    use super::{PROJECT_NAME, PROJECT_SLUG, USER_AGENT};

    #[test]
    fn project_slug_matches_project_name() {
        assert_eq!(PROJECT_NAME.to_ascii_lowercase(), PROJECT_SLUG);
    }

    #[test]
    fn user_agent_mentions_project_name() {
        assert!(USER_AGENT.contains(PROJECT_NAME));
    }
}
