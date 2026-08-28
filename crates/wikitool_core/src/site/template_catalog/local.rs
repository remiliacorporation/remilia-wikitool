use super::*;

pub(super) fn load_local_templates(
    paths: &ResolvedPaths,
) -> Result<(LocalTemplateMap, TemplateAliasMap)> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;

    let mut local_templates = LocalTemplateMap::new();
    let mut documentation_pages = BTreeMap::<String, Vec<LocalDocumentationPage>>::new();
    let mut redirect_aliases = TemplateAliasMap::new();
    for file in files {
        if file.namespace != "Template" {
            continue;
        }
        let normalized_title = normalize_template_lookup_title(&file.title);
        if normalized_title.is_empty() {
            continue;
        }
        if file.is_redirect {
            if let Some(target) = file.redirect_target.as_deref() {
                let normalized_target = normalize_template_lookup_title(target);
                if !normalized_target.is_empty() && normalized_target != normalized_title {
                    redirect_aliases
                        .entry(normalized_target)
                        .or_default()
                        .push(file.title);
                }
            }
            continue;
        }
        let full_path = relative_path_to_path(paths, &file.relative_path);
        let content = fs::read_to_string(&full_path)
            .with_context(|| format!("failed to read {}", full_path.display()))?;
        let relative_path = file.relative_path.clone();

        if let Some((base_title, subpage)) = normalized_title.split_once('/') {
            if is_documentation_subpage(subpage) {
                documentation_pages
                    .entry(base_title.to_string())
                    .or_default()
                    .push(LocalDocumentationPage {
                        title: normalized_title,
                        relative_path,
                        content,
                    });
            }
            continue;
        }

        let templatedata = extract_template_data(&content)?;
        let declared_parameter_keys = extract_source_parameters(&content);
        let module_titles = extract_module_references(&content);
        let mut local_examples = extract_template_examples(
            &content,
            &normalized_title,
            &normalized_title,
            &file.relative_path,
            4,
        );
        let summary_text = templatedata
            .as_ref()
            .and_then(|item| item.description.clone())
            .or_else(|| extract_summary_text(&content));
        local_templates.insert(
            normalized_title.clone(),
            LocalTemplateRecord {
                template_title: normalized_title,
                relative_path: relative_path.clone(),
                category: relative_template_category(&relative_path),
                templatedata,
                summary_text,
                declared_parameter_keys,
                documentation_pages: Vec::new(),
                local_examples: std::mem::take(&mut local_examples),
                module_titles,
            },
        );
    }

    for (base_title, pages) in documentation_pages {
        if let Some(entry) = local_templates.get_mut(&base_title) {
            for page in pages {
                if entry.summary_text.is_none() {
                    entry.summary_text = extract_summary_text(&page.content);
                }
                entry.local_examples.extend(extract_template_examples(
                    &page.content,
                    &base_title,
                    &page.title,
                    &page.relative_path,
                    4,
                ));
                entry.documentation_pages.push(page);
            }
        }
    }

    Ok((local_templates, redirect_aliases))
}

fn is_documentation_subpage(subpage: &str) -> bool {
    matches!(
        subpage.to_ascii_lowercase().as_str(),
        "doc" | "documentation"
    )
}

fn relative_template_category(relative_path: &str) -> String {
    let normalized = normalize_path(relative_path);
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.first().copied() == Some("templates") && segments.len() >= 2 {
        return segments[1].to_string();
    }
    if !segments.is_empty() {
        return segments[0].to_string();
    }
    "templates".to_string()
}

fn relative_path_to_path(paths: &ResolvedPaths, relative_path: &str) -> PathBuf {
    let mut path = paths.project_root.clone();
    for segment in normalize_path(relative_path).split('/') {
        if !segment.is_empty() {
            path.push(segment);
        }
    }
    path
}
