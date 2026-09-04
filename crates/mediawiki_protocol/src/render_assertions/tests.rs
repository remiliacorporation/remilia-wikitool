use super::*;

fn assertion(value: serde_json::Value) -> RenderDomAssertion {
    serde_json::from_value(value).unwrap()
}

#[test]
fn assertions_check_every_scope_and_every_match() {
    let assertions = [assertion(serde_json::json!({
        "selector": "img", "attributes": {"alt": null}, "min_count": 1, "max_count": 2
    }))];
    let results = analyze_dom_assertions(
        "<aside class='box'><img alt='Portrait'></aside><aside class='box'><img alt=''><img></aside>",
        Some("box"),
        &assertions,
    );
    assert_eq!(results.len(), 2);
    assert!(results[0].passed);
    assert!(!results[1].passed);
    assert_eq!(results[1].mismatched_count, 1);
}

#[test]
fn scope_root_entities_roles_headings_and_negative_selectors() {
    let assertions = [
        assertion(
            serde_json::json!({"selector": "aside", "attributes": {"role": "note", "aria-label": "A & B"}, "max_count": 1}),
        ),
        assertion(serde_json::json!({"selector": "h2", "text_contains": "A & B", "max_count": 1})),
        assertion(
            serde_json::json!({"selector": "img:not([alt])", "min_count": 0, "max_count": 0}),
        ),
    ];
    validate_dom_assertions(&assertions).unwrap();
    let results = analyze_dom_assertions(
        "<aside class='box' role='note' aria-label='A &amp; B'><h2>A <em>&amp;</em> B</h2><img alt=''></aside>",
        Some("box"),
        &assertions,
    );
    assert!(results.iter().all(|result| result.passed));
    assert_eq!(results[0].matched_count, 1);
}

#[test]
fn absent_scope_is_not_vacuous_success_and_outside_matches_do_not_count() {
    let assertions = [assertion(
        serde_json::json!({"selector": "a", "min_count": 0, "max_count": 0}),
    )];
    assert!(!analyze_dom_assertions("<a>Outside</a>", Some("box"), &assertions)[0].passed);
    assert!(
        analyze_dom_assertions(
            "<a>Outside</a><div class='box'></div>",
            Some("box"),
            &assertions
        )[0]
        .passed
    );
}

#[test]
fn invalid_assertions_refuse_before_network() {
    for value in [
        serde_json::json!({"selector": "["}),
        serde_json::json!({"selector": "img", "max_count": 0}),
        serde_json::json!({"selector": "h2", "text_contains": "  "}),
        serde_json::json!({"selector": "h2", "attributes": {"bad name": null}}),
    ] {
        assert!(validate_dom_assertions(&[assertion(value)]).is_err());
    }
    assert!(
        serde_json::from_value::<RenderDomAssertion>(
            serde_json::json!({"selector": "h2", "viewport": 320})
        )
        .is_err()
    );
}
