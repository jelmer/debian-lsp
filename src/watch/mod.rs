pub mod completion;
pub mod detection;
pub mod fields;
pub mod folding;
pub mod semantic;

pub use completion::*;
pub use detection::is_watch_file;
pub use folding::generate_folding_ranges;
pub use semantic::generate_semantic_tokens;
