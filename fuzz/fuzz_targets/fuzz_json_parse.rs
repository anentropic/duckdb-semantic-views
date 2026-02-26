#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic regardless of input.
        // Errors are fine -- panics/UB are not.
        let _ = semantic_views::model::SemanticViewDefinition::from_json("fuzz_test", s);
    }
});
