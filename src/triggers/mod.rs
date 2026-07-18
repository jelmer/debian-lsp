pub mod completion;
pub mod detection;
pub mod parser;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_triggers_file;
pub use semantic::generate_semantic_tokens;
