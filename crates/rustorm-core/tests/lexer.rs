//! The shared syntax lexer (step 3 of plan basilisk).

use proptest::prelude::*;
use rustorm_core::{lex, lex_document, Lexer, SpanKind};

fn assert_covers(line: &str, spans: &[rustorm_core::Span]) {
    let mut at = 0;
    for s in spans {
        assert_eq!(s.start, at, "gap or overlap at byte {at} in {line:?}");
        assert!(s.end > s.start, "empty span in {line:?}");
        at = s.end;
    }
    assert_eq!(at, line.len(), "span coverage stops short in {line:?}");
}

fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Host".to_string()),
        Just("host".to_string()),
        Just("ProxyCommand".to_string()),
        Just("proxyjump".to_string()),
        Just("HostName".to_string()),
        Just("#".to_string()),
        Just("=".to_string()),
        Just(" ".to_string()),
        Just("\t".to_string()),
        Just("\"".to_string()),
        Just("#---------#".to_string()),
        Just("section: x".to_string()),
        Just("é".to_string()),
        Just("日本".to_string()),
        "[a-zA-Z0-9%@:._*-]{1,8}",
    ]
}

fn generated_line() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(piece(), 0..10),
        prop_oneof![Just(""), Just("\n"), Just("\r\n")],
    )
        .prop_map(|(parts, eol)| parts.concat() + eol)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn every_byte_is_covered_by_exactly_one_span(line in generated_line(), in_banner in any::<bool>()) {
        assert_covers(&line, &rustorm_core::lex_line(&line, in_banner));
    }
}

#[test]
fn proxy_kinds_are_distinct() {
    let line = "    ProxyCommand ssh -W %h:%p bastion\n";
    let kinds: Vec<SpanKind> = lex(line).iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&SpanKind::ProxyCommand));
    let line = "ProxyJump=jump1,jump2";
    let spans = lex(line);
    let last = spans.last().unwrap();
    assert_eq!(last.kind, SpanKind::ProxyJump);
    assert_eq!(&line[last.start..last.end], "jump1,jump2");
}

#[test]
fn whole_banner_is_banner_kind_and_document_offsets_are_absolute() {
    let text = include_str!("fixtures/banner-data-foundry.txt").to_string() + "Host a\n";
    let spans = lex_document(&text);
    assert_covers(&text, &spans);
    let banner_end = include_str!("fixtures/banner-data-foundry.txt").len();
    for s in spans.iter().filter(|s| s.end <= banner_end) {
        assert!(
            matches!(s.kind, SpanKind::Banner | SpanKind::Whitespace),
            "{s:?}"
        );
    }
    assert!(spans
        .iter()
        .any(|s| s.kind == SpanKind::HostName && &text[s.start..s.end] == "a"));
    let mut lx = Lexer::new();
    for line in include_str!("fixtures/basic.conf").split_inclusive('\n') {
        assert_covers(line, &lx.next_line(line));
    }
}
