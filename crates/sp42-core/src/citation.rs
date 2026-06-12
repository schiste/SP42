//! Citation support: the Citoid bibliographic-metadata client and citation
//! URL helpers, lifted verbatim from the `impl/citation-verification` branch.
//!
//! Only the `citoid` and `urls` submodules exist on this branch; the full
//! citation-verification module set arrives when that branch merges. This
//! declaration file then takes that branch's version (a known take-theirs
//! conflict, recorded in the bare-URL repair design plan).

// The lifted files stay byte-identical to the source branch, so their doc
// comments may link to sibling modules that only exist there; the allows live
// here, in the already-divergent declaration file, and disappear with the
// take-theirs resolution when the full module set lands.
#[allow(rustdoc::broken_intra_doc_links)]
pub mod citoid;
#[allow(rustdoc::broken_intra_doc_links)]
pub mod urls;
