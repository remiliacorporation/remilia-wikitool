use super::*;

fn spec() -> TemplateMigrationSpec {
    TemplateMigrationSpec {
        schema: "template_migration_spec_v1".into(),
        from_template: "Template:Old".into(),
        to_template: "Template:New".into(),
        title_case: MigrationTitleCase::FirstLetter,
        parameter_renames: BTreeMap::from([("old".into(), "new".into())]),
        deprecated_parameters: vec!["obsolete".into()],
    }
}

fn analyze(source: &str) -> TemplateMigrationFile {
    analyze_migration_source(
        source,
        "Article".into(),
        "wiki_content/Main/Article.wiki".into(),
        &spec(),
    )
}

fn candidate(source: &str, report: &TemplateMigrationFile) -> String {
    let mut result = source.to_string();
    for patch in report.patches.iter().rev() {
        assert_eq!(&source[patch.start_byte..patch.end_byte], patch.before);
        result.replace_range(patch.start_byte..patch.end_byte, &patch.after);
    }
    assert_eq!(
        report.candidate_sha256.as_deref(),
        Some(compute_sha256(&result).as_str())
    );
    result
}

#[test]
fn exact_spans_preserve_unicode_spacing_nested_values_and_literals() {
    let source = "é {{ Old\n | old = [[A|B=C]] | x={{Old|old=<nowiki>|=</nowiki>}} }} <!-- {{Old}} --> <syntaxhighlight>{{Old}}</syntaxhighlight>";
    let report = analyze(source);
    assert!(report.review_reasons.is_empty());
    assert_eq!(report.invocations.len(), 2);
    assert_eq!(
        candidate(source, &report),
        "é {{ Template:New\n | new = [[A|B=C]] | x={{Template:New|new=<nowiki>|=</nowiki>}} }} <!-- {{Old}} --> <syntaxhighlight>{{Old}}</syntaxhighlight>"
    );
    for invocation in report.invocations {
        assert_eq!(
            compute_sha256(&source[invocation.start_byte..invocation.end_byte]),
            invocation.source_sha256
        );
    }
}

#[test]
fn collisions_and_ambiguity_withhold_the_entire_file() {
    for source in [
        "{{Old|old=a|new=b}}",
        "{{Old|old=a|old=b}}",
        "{{Old|obsolete=keep this}}",
        "{{Old|{{{key}}}=value}}",
        "{{Old|old=valid}} {{Old",
        "{{Old|old=valid}} {{{{{which}}}|x=1}}",
    ] {
        let report = analyze(source);
        assert!(!report.review_reasons.is_empty(), "{source}");
        assert!(report.patches.is_empty(), "{source}");
        assert!(report.candidate_sha256.is_none(), "{source}");
    }
}

#[test]
fn positional_collisions_count_empty_slots_and_explicit_numbers() {
    let mut rule = spec();
    rule.parameter_renames = BTreeMap::from([("old".into(), "2".into())]);
    let report = analyze_migration_source(
        "{{Old||second|old=third}}",
        "Article".into(),
        "a.wiki".into(),
        &rule,
    );
    assert!(
        report
            .review_reasons
            .iter()
            .any(|reason| reason.contains("parameter_collision"))
    );
    assert_eq!(report.invocations[0].parameter_keys, ["1", "2", "old"]);
    rule.parameter_renames = BTreeMap::from([("2".into(), "new".into())]);
    let report =
        analyze_migration_source("{{Old||second}}", "Article".into(), "a.wiki".into(), &rule);
    assert!(
        report
            .review_reasons
            .iter()
            .any(|reason| reason.contains("positional_rename"))
    );
}

#[test]
fn title_case_and_namespace_are_explicit() {
    let source = "{{old}} {{Old}} {{OLD}} {{:Old}} {{:Template:Old}} {{subst:Old}}";
    assert_eq!(analyze(source).invocations.len(), 3);
    let mut rule = spec();
    rule.title_case = MigrationTitleCase::CaseSensitive;
    assert_eq!(
        analyze_migration_source(source, "A".into(), "a.wiki".into(), &rule)
            .invocations
            .len(),
        2
    );
    rule.to_template = "New|injected=1".into();
    assert!(rule.validate().is_err());
}

#[test]
fn unchanged_invocations_remain_enumerated_for_retirement_review() {
    let mut rule = spec();
    rule.to_template = rule.from_template.clone();
    rule.parameter_renames.clear();
    let report = analyze_migration_source("{{Old|safe=x}}", "A".into(), "a.wiki".into(), &rule);
    assert_eq!(report.invocations.len(), 1);
    assert!(report.patches.is_empty());
    assert!(report.review_reasons.is_empty());
}
