use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Info,
    Success,
    Warning,
    Accent,
    Danger,
}

#[component]
pub fn StatusBadge(label: String, tone: StatusTone) -> impl IntoView {
    let (background, foreground, border) = tone_colors(tone);

    view! {
        <span
            class="sp42-status-badge"
            style=format!(
                "display:inline-flex;align-items:center;gap:4px;padding:4px 10px;border-radius:var(--radius-sm);border:1px solid {border};background:{background};color:{foreground};font-size:.78rem;line-height:1.1;font-weight:700;letter-spacing:.02em;text-transform:uppercase;"
            )
        >
            {label}
        </span>
    }
}

/// Tone -> (background, foreground, border) as CSS custom property references,
/// so badges follow the active Codex theme. Token values live in
/// `sp42-ui/static/style.css` (`--tone-*`).
#[must_use]
pub fn tone_colors(tone: StatusTone) -> (&'static str, &'static str, &'static str) {
    match tone {
        StatusTone::Neutral => (
            "var(--tone-neutral-bg)",
            "var(--tone-neutral-text)",
            "var(--tone-neutral-border)",
        ),
        StatusTone::Info => (
            "var(--tone-info-bg)",
            "var(--tone-info-text)",
            "var(--tone-info-border)",
        ),
        StatusTone::Success => (
            "var(--tone-positive-bg)",
            "var(--tone-positive-text)",
            "var(--tone-positive-border)",
        ),
        StatusTone::Warning => (
            "var(--tone-caution-bg)",
            "var(--tone-caution-text)",
            "var(--tone-caution-border)",
        ),
        StatusTone::Accent => (
            "var(--accent-bg)",
            "var(--tone-info-text)",
            "var(--accent-border)",
        ),
        StatusTone::Danger => (
            "var(--danger-bg)",
            "var(--danger-light)",
            "var(--danger-border)",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusTone, tone_colors};

    #[test]
    fn tone_colors_are_distinct() {
        let neutral = tone_colors(StatusTone::Neutral);
        let accent = tone_colors(StatusTone::Accent);

        assert_ne!(neutral.0, accent.0);
        assert_ne!(neutral.1, accent.1);
        assert_ne!(neutral.2, accent.2);
    }
}
