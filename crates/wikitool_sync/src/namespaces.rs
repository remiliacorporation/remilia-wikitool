pub(super) fn namespace_name_to_id(namespace: &str) -> Option<i32> {
    mediawiki_protocol::namespace::namespace_name_to_id(namespace)
}

pub(super) fn is_template_namespace_id(namespace: i32) -> bool {
    mediawiki_protocol::namespace::is_template_namespace_id(namespace)
}
