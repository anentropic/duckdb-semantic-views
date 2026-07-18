#![no_main]
use libfuzzer_sys::fuzz_target;
use semantic_views::model::SemanticViewDefinition;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic regardless of input.
        // Errors are fine -- panics/UB are not.
        if let Ok(def) = SemanticViewDefinition::from_yaml("fuzz_test", s) {
            // Serde round-trip oracle: a definition that parsed must
            // re-serialize and re-parse to an equal value. `from_yaml` is a
            // thin wrapper over `yaml_serde::from_str`, and every field's
            // `skip_serializing_if` matches its `default`, so serialize ->
            // deserialize is the identity.
            let reserialized =
                yaml_serde::to_string(&def).expect("serialize parsed def back to YAML");
            let reparsed = SemanticViewDefinition::from_yaml("fuzz_test", &reserialized)
                .expect("re-parse of serialized def must succeed");
            assert_eq!(
                def, reparsed,
                "YAML serde round-trip changed the definition"
            );
        }
    }
});
