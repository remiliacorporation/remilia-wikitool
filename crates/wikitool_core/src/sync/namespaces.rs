pub(super) fn namespace_name_to_id(namespace: &str) -> Option<i32> {
    crate::mw::namespace::namespace_name_to_id(namespace)
}

pub(super) fn is_template_namespace_id(namespace: i32) -> bool {
    crate::mw::namespace::is_template_namespace_id(namespace)
}

#[cfg(test)]
pub(crate) use crate::mw::siteinfo::SiteInfoNamespace;

#[cfg(test)]
pub(crate) fn should_include_discovered_namespace(namespace: &SiteInfoNamespace) -> bool {
    crate::mw::namespace::should_include_discovered_namespace(namespace)
}

#[cfg(test)]
pub(crate) fn namespace_display_name(namespace: &SiteInfoNamespace) -> Option<String> {
    crate::mw::namespace::namespace_display_name(namespace)
}
