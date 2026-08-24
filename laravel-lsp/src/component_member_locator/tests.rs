use super::*;

#[test]
fn locates_a_method_declaration() {
    let source = r#"<?php
class ContractViewPage extends Page
{
    public function checkPrefillStatus(): void
    {
    }
}
"#;
    let loc = locate_member(source, "checkPrefillStatus").expect("method found");
    assert_eq!(loc.kind, MemberKind::Method);
    // Line 3 (0-based) is `    public function checkPrefillStatus(): void`.
    assert_eq!(loc.line, 3);
    let name = &source.lines().nth(3).unwrap()[loc.start_column as usize..loc.end_column as usize];
    assert_eq!(name, "checkPrefillStatus");
}

#[test]
fn locates_a_property_declaration_and_skips_the_dollar() {
    let source = r#"<?php
class ContractViewPage extends Page
{
    public ?string $prefillStatus = null;
}
"#;
    let loc = locate_member(source, "prefillStatus").expect("property found");
    assert_eq!(loc.kind, MemberKind::Property);
    assert_eq!(loc.line, 3);
    let line = source.lines().nth(3).unwrap();
    let name = &line[loc.start_column as usize..loc.end_column as usize];
    assert_eq!(name, "prefillStatus");
    // The `$` sigil sits immediately before the located range.
    assert_eq!(
        &line[loc.start_column as usize - 1..loc.start_column as usize],
        "$"
    );
}

#[test]
fn locates_a_promoted_constructor_property() {
    let source = r#"<?php
class ContractService
{
    public function __construct(public readonly ContractRepository $repository)
    {
    }
}
"#;
    let loc = locate_member(source, "repository").expect("promoted property found");
    assert_eq!(loc.kind, MemberKind::Property);
    let line = source.lines().nth(3).unwrap();
    let name = &line[loc.start_column as usize..loc.end_column as usize];
    assert_eq!(name, "repository");
}

#[test]
fn returns_none_when_no_member_matches() {
    let source = r#"<?php
class ContractViewPage extends Page
{
    public ?string $prefillStatus = null;

    public function checkPrefillStatus(): void
    {
    }
}
"#;
    assert!(locate_member(source, "doesNotExist").is_none());
}

#[test]
fn returns_none_for_unparseable_source() {
    // Not actually invalid PHP as far as tree-sitter's concerned, but there's
    // no member declaration matching — same shape as `returns_none_when_no_member_matches`,
    // just confirms an empty/degenerate file doesn't panic.
    assert!(locate_member("", "anything").is_none());
}

#[test]
fn public_action_method_names_skips_lifecycle_and_non_public() {
    let source = r#"<?php
class ContractViewPage {
    public function mount(?string $id = null): void {}
    public function render() {}
    public function updatedContractData(): void {}
    public function checkPrefillStatus(): void {}
    public function enterEditMode(): void {}
    public static function staticHelper(): void {}
    protected function internal(): void {}
    public function __get($name) {}
}
"#;
    assert_eq!(
        public_action_method_names(source),
        vec![
            "checkPrefillStatus".to_string(),
            "enterEditMode".to_string()
        ]
    );
}

#[test]
fn public_property_types_includes_scalars_and_untyped() {
    let source = r#"<?php
class Page {
    public string $prefillStatus = 'none';
    public ?string $contractId = null;
    public $legacy;
    protected string $hidden = '';
    public function __construct(public int $count) {}
}
"#;
    let props = public_property_types(source);
    let names: Vec<&str> = props.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"prefillStatus"));
    assert!(names.contains(&"contractId"));
    assert!(names.contains(&"legacy"));
    assert!(names.contains(&"count"));
    assert!(!names.contains(&"hidden"));
    assert_eq!(
        props.iter().find(|(n, _)| n == "contractId").unwrap().1,
        "string"
    );
    assert_eq!(
        props.iter().find(|(n, _)| n == "legacy").unwrap().1,
        "mixed"
    );
}
