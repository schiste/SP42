use sp42_core::{RenderedHunkPreview, StructuredDiff};

#[derive(Debug, Clone)]
pub(crate) struct RevisionArtifacts {
    pub(crate) diff: StructuredDiff,
    pub(crate) media_diff: Option<sp42_core::MediaDiffReport>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedRevisionArtifacts {
    pub(crate) fetched_at_ms: i64,
    pub(crate) artifacts: RevisionArtifacts,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedRenderedHunkPreview {
    pub(crate) fetched_at_ms: i64,
    pub(crate) preview: RenderedHunkPreview,
}
