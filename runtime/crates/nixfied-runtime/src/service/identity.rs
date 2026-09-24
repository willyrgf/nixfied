/// A session-local service reference. Length framing keeps arbitrary run IDs
/// distinct without hashing configuration or inventing reusable service identity.
pub(crate) fn service_instance_id(run_id: &str, service_name: &str) -> String {
    format!("{}:{run_id}{service_name}", run_id.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_service_references_preserve_both_components() {
        assert_eq!(service_instance_id("run", "db"), "3:rundb");
        assert_ne!(
            service_instance_id("run", "db"),
            service_instance_id("other", "db")
        );
        assert_ne!(
            service_instance_id("run", "db"),
            service_instance_id("run", "worker")
        );
        assert_ne!(
            service_instance_id("a:b", "c"),
            service_instance_id("a", "b:c")
        );
        assert_ne!(
            service_instance_id("ab", "c"),
            service_instance_id("a", "bc")
        );
    }
}
