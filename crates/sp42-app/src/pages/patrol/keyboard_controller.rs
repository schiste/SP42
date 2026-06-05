use leptos::prelude::*;
use sp42_core::{QueuedEdit, SessionActionKind};

pub(super) fn handle_patrol_keydown(
    event: leptos::ev::KeyboardEvent,
    set_action_trigger: WriteSignal<Option<SessionActionKind>>,
    set_skip_trigger: WriteSignal<bool>,
    selected_index: Memo<usize>,
    queue: Memo<Vec<QueuedEdit>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    set_show_backoffice: WriteSignal<bool>,
    set_show_help: WriteSignal<bool>,
) {
    if is_text_entry_event(&event) {
        return;
    }

    match event.key().as_str() {
        "r" | "R" => set_action_trigger.set(Some(SessionActionKind::Rollback)),
        "u" | "U" => set_action_trigger.set(Some(SessionActionKind::Undo)),
        "p" | "P" => set_action_trigger.set(Some(SessionActionKind::Patrol)),
        "s" | "S" => set_skip_trigger.set(true),
        "ArrowUp" => {
            event.prevent_default();
            let idx = selected_index.get();
            let queue = queue.get();
            if idx > 0 {
                if let Some(prev) = queue.get(idx - 1) {
                    set_selected_rev_id.set(Some(prev.event.rev_id));
                }
            }
        }
        "ArrowDown" => {
            event.prevent_default();
            let idx = selected_index.get();
            let queue = queue.get();
            if let Some(next) = queue.get(idx + 1) {
                set_selected_rev_id.set(Some(next.event.rev_id));
            }
        }
        "D" if event.ctrl_key() && event.shift_key() => {
            event.prevent_default();
            set_show_backoffice.update(|visible| *visible = !*visible);
        }
        "?" => set_show_help.set(true),
        "Escape" => {
            set_show_help.set(false);
            set_show_backoffice.set(false);
        }
        _ => {}
    }
}

fn is_text_entry_event(event: &leptos::ev::KeyboardEvent) -> bool {
    let tag = event
        .target()
        .and_then(|target| {
            use wasm_bindgen::JsCast;
            target.dyn_into::<web_sys::Element>().ok()
        })
        .map(|element| element.tag_name());
    matches!(tag.as_deref(), Some("INPUT") | Some("TEXTAREA"))
}
