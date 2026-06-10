//! Citation support: the Citoid bibliographic-metadata client and citation
//! URL helpers, lifted verbatim from the `impl/citation-verification` branch.
//!
//! Only the `citoid` and `urls` submodules exist on this branch; the full
//! citation-verification module set arrives when that branch merges. This
//! declaration file then takes that branch's version (a known take-theirs
//! conflict, recorded in the bare-URL repair design plan).

pub mod citoid;
pub mod urls;
