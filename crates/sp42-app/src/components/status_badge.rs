use leptos::prelude::*;

pub use sp42_ui::StatusTone;

#[component]
pub fn StatusBadge(label: String, tone: StatusTone) -> impl IntoView {
    sp42_ui::StatusBadge(sp42_ui::StatusBadgeProps::new(label).with_tone(tone))
}
