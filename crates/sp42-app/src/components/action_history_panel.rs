use leptos::prelude::*;

use crate::platform::debug::{
    DevActionHistoryRecord, dev_action_history_lines, fetch_dev_action_history,
};

use super::{StatusTone, TelemetryPanel};

#[component]
pub fn ActionHistoryPanel(refresh_tick: ReadSignal<u64>) -> impl IntoView {
    let (history, set_history) = signal(None::<Vec<DevActionHistoryRecord>>);
    let (status_message, set_status_message) =
        signal("No action history refresh run yet.".to_string());

    let refresh_action = Action::new_local(move |_: &()| {
        let set_history = set_history;
        let set_status_message = set_status_message;
        async move {
            match fetch_dev_action_history().await {
                Ok(records) => {
                    set_status_message
                        .set(format!("Loaded {} action history item(s).", records.len()));
                    set_history.set(Some(records));
                }
                Err(error) => {
                    set_history.set(None);
                    set_status_message.set(format!("Action history unavailable: {error}"));
                }
            }
        }
    });

    Effect::new(move |_| {
        let _ = refresh_tick.get();
        refresh_action.dispatch_local(());
    });

    view! {
        <section style="display:grid;gap:10px;">
            <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;">
                <div>
                    <h2 style="margin:0;">"Action History"</h2>
                    <p style="margin:.25rem 0 0;color:#8b9fc0;">
                        "A compact live trail for action records from the localhost bridge."
                    </p>
                </div>
                <button
                    on:click=move |_| {
                        refresh_action.dispatch_local(());
                    }
                    disabled=move || refresh_action.pending().get()
                >
                    "Refresh History"
                </button>
            </div>
            <p style="margin:0;color:#8b9fc0;">{move || status_message.get()}</p>
            {move || {
                let records = history
                    .get()
                    .unwrap_or_default();
                let badges = vec![
                    (
                        format!("{} item(s)", records.len()),
                        if records.is_empty() {
                            StatusTone::Warning
                        } else {
                            StatusTone::Success
                        },
                    ),
                    (
                        if history.get().is_some() {
                            "Live".to_string()
                        } else {
                            "Waiting".to_string()
                        },
                        if history.get().is_some() {
                            StatusTone::Info
                        } else {
                            StatusTone::Warning
                        },
                    ),
                ];

                view! {
                    <TelemetryPanel
                        title="Action History".to_string()
                        description="Live action records fetched from the localhost bridge.".to_string()
                        tone=StatusTone::Accent
                        badges=badges
                        lines=dev_action_history_lines(&records)
                    />
                }
            }}
        </section>
    }
}
