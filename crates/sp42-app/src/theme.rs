//! Dark/light theming mapped onto the Wikimedia Codex token sets.
//!
//! Dark is the default. The choice persists in `localStorage` under
//! `sp42-theme` and is applied as a `data-theme` attribute on the document
//! root. A small inline script in `index.html` applies the saved theme before
//! first paint (avoiding a flash); this module keeps the in-app toggle in sync
//! and re-persists on change.

const STORAGE_KEY: &str = "sp42-theme";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    /// Value written to `data-theme` / `localStorage`.
    fn attr(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The opposite theme — what the toggle switches to.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Label naming the theme this control switches *to* (not the current one).
    #[must_use]
    pub fn other_label(self) -> &'static str {
        match self {
            Self::Dark => "Light",
            Self::Light => "Dark",
        }
    }
}

/// Theme saved from a previous session, or [`Theme::Dark`] by default.
#[must_use]
pub fn stored_theme() -> Theme {
    let stored = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten());

    match stored.as_deref() {
        Some("light") => Theme::Light,
        _ => Theme::Dark,
    }
}

/// Apply `theme` to the document root (`data-theme`) and persist the choice.
pub fn apply_theme(theme: Theme) {
    let Some(window) = web_sys::window() else {
        return;
    };

    if let Some(root) = window.document().and_then(|doc| doc.document_element()) {
        let _ = root.set_attribute("data-theme", theme.attr());
    }

    if let Ok(Some(storage)) = window.local_storage() {
        let _ = storage.set_item(STORAGE_KEY, theme.attr());
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn toggled_is_involutive() {
        assert_eq!(Theme::Dark.toggled(), Theme::Light);
        assert_eq!(Theme::Light.toggled(), Theme::Dark);
        assert_eq!(Theme::Dark.toggled().toggled(), Theme::Dark);
    }

    #[test]
    fn other_label_names_the_target_theme() {
        assert_eq!(Theme::Dark.other_label(), "Light");
        assert_eq!(Theme::Light.other_label(), "Dark");
    }
}
