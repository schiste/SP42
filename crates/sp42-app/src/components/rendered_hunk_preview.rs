use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use sp42_core::RenderedHunkPreview;

use crate::platform::live::fetch_rendered_hunk;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RenderedHunkContext {
    wiki_id: String,
    rev_id: u64,
    old_rev_id: u64,
}

impl RenderedHunkContext {
    pub(crate) fn new(wiki_id: String, rev_id: u64, old_rev_id: u64) -> Self {
        Self {
            wiki_id,
            rev_id,
            old_rev_id,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RenderedHunkPreviewController {
    expanded: ReadSignal<HashSet<usize>>,
    set_expanded: WriteSignal<HashSet<usize>>,
    cache: ReadSignal<HashMap<usize, RenderedHunkPreview>>,
    set_cache: WriteSignal<HashMap<usize, RenderedHunkPreview>>,
    loading: ReadSignal<HashSet<usize>>,
    set_loading: WriteSignal<HashSet<usize>>,
    errors: ReadSignal<HashMap<usize, String>>,
    set_errors: WriteSignal<HashMap<usize, String>>,
}

pub(crate) fn create_rendered_hunk_preview_controller() -> RenderedHunkPreviewController {
    let (expanded, set_expanded) = signal(HashSet::<usize>::new());
    let (cache, set_cache) = signal(HashMap::<usize, RenderedHunkPreview>::new());
    let (loading, set_loading) = signal(HashSet::<usize>::new());
    let (errors, set_errors) = signal(HashMap::<usize, String>::new());

    RenderedHunkPreviewController {
        expanded,
        set_expanded,
        cache,
        set_cache,
        loading,
        set_loading,
        errors,
        set_errors,
    }
}

impl RenderedHunkPreviewController {
    pub(crate) fn is_expanded(self, hunk_index: usize) -> bool {
        self.expanded.get().contains(&hunk_index)
    }

    pub(crate) fn is_loading(self, hunk_index: usize) -> bool {
        self.loading.get().contains(&hunk_index)
    }

    pub(crate) fn error(self, hunk_index: usize) -> Option<String> {
        self.errors.get().get(&hunk_index).cloned()
    }

    pub(crate) fn preview(self, hunk_index: usize) -> Option<RenderedHunkPreview> {
        self.cache.get().get(&hunk_index).cloned()
    }

    pub(crate) fn toggle(self, context: RenderedHunkContext, hunk_index: usize) {
        let is_expanded = self.expanded.get_untracked().contains(&hunk_index);
        let mut expanded = self.expanded.get_untracked();
        if is_expanded {
            expanded.remove(&hunk_index);
            self.set_expanded.set(expanded);
            return;
        }

        expanded.insert(hunk_index);
        self.set_expanded.set(expanded);

        if self.cache.get_untracked().contains_key(&hunk_index)
            || self.loading.get_untracked().contains(&hunk_index)
        {
            return;
        }

        let RenderedHunkContext {
            wiki_id,
            rev_id,
            old_rev_id,
        } = context;
        let loading = self.loading;
        let set_loading = self.set_loading;
        let cache = self.cache;
        let set_cache = self.set_cache;
        let errors = self.errors;
        let set_errors = self.set_errors;

        wasm_bindgen_futures::spawn_local(async move {
            let mut loading_state = loading.get_untracked();
            loading_state.insert(hunk_index);
            set_loading.set(loading_state);

            match fetch_rendered_hunk(&wiki_id, rev_id, old_rev_id, hunk_index).await {
                Ok(Some(preview)) => {
                    let mut cache_state = cache.get_untracked();
                    cache_state.insert(hunk_index, preview);
                    set_cache.set(cache_state);
                    let mut error_state = errors.get_untracked();
                    error_state.remove(&hunk_index);
                    set_errors.set(error_state);
                }
                Ok(None) => {
                    let mut error_state = errors.get_untracked();
                    error_state.insert(
                        hunk_index,
                        "No rendered preview is available for this hunk.".to_string(),
                    );
                    set_errors.set(error_state);
                }
                Err(error) => {
                    let mut error_state = errors.get_untracked();
                    error_state.insert(hunk_index, error);
                    set_errors.set(error_state);
                }
            }

            let mut loading_state = loading.get_untracked();
            loading_state.remove(&hunk_index);
            set_loading.set(loading_state);
        });
    }
}
