use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Info,
    Success,
    Warning,
    Accent,
}

#[component]
pub fn StatusBadge(label: String, tone: StatusTone) -> impl IntoView {
    let (background, foreground, border) = tone_colors(tone);

    view! {
        <span
            class="sp42-status-badge"
            style=format!(
                "display:inline-flex;align-items:center;gap:4px;padding:4px 10px;border-radius:4px;border:1px solid {border};background:{background};color:{foreground};font-size:.78rem;line-height:1.1;font-weight:700;letter-spacing:.02em;text-transform:uppercase;"
            )
        >
            {label}
        </span>
    }
}

#[must_use]
pub fn tone_colors(tone: StatusTone) -> (&'static str, &'static str, &'static str) {
    match tone {
        StatusTone::Neutral => (
            "rgba(255,255,255,0.08)",
            "#dce4f2",
            "rgba(220,228,242,0.25)",
        ),
        StatusTone::Info => ("rgba(63,127,255,0.16)", "#d6e4ff", "rgba(63,127,255,0.35)"),
        StatusTone::Success => ("rgba(61,185,125,0.16)", "#dff8e9", "rgba(61,185,125,0.36)"),
        StatusTone::Warning => ("rgba(224,160,0,0.18)", "#fff0bf", "rgba(224,160,0,0.38)"),
        StatusTone::Accent => (
            "rgba(143,183,255,0.14)",
            "#d6e4ff",
            "rgba(143,183,255,0.35)",
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
