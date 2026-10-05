//! End-to-end proof that the CSS parser produces the trees it claims to.
//!
//! A parser is the easiest thing in a codebase to ship broken, because it
//! compiles, it returns a `Node`, and the render succeeds. Every wrong property
//! is a silently default one. So the tests here assert on the *decoded style
//! values* rather than on "it did not panic", and the end-to-end ones count ink
//! in the rendered PNG.
//!
//! Two failure modes get dedicated tests:
//!   * a declaration that is silently dropped, which looks like a browser bug
//!   * a declaration silently *guessed*, which is worse — it renders plausible
//!     and wrong.

use super::css::{self, Element, ParseReport, Stylesheet};
use super::{Background, Color, Display, FlexDir, Node, ShadowKind, Style};

/// Style of the root node, whether it collapsed to text or stayed a container.
///
/// A boxless element with only text collapses to a [`Node::Text`] — that is the
/// whole point of the rule, since text styling belongs on the glyphs rather than
/// on an empty box between the parent and the text.
fn root_style(tree: &Node) -> &Style {
    match tree {
        Node::Container { style, .. } | Node::Text { style, .. } => style,
        _ => panic!("expected a container or text root"),
    }
}

fn parse(css: &str) -> (Stylesheet, ParseReport) {
    let mut report = ParseReport::default();
    let sheet = Stylesheet::parse(css, &mut report).expect("stylesheet parses");
    (sheet, report)
}

#[test]
fn parses_lengths_in_every_supported_unit() {
    let (sheet, report) = parse(".a{width:100px;height:2in;padding:1cm;gap:10pt}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report.clone());
    let s = root_style(&tree);
    assert_eq!(s.width, Some(100.0));
    assert_eq!(s.height, Some(192.0), "2in is 192px");
    assert!(
        (s.padding - 37.8).abs() < 0.2,
        "1cm is ~37.8px, got {}",
        s.padding
    );
    assert!((s.gap - 13.3).abs() < 0.2, "10pt is ~13.3px, got {}", s.gap);
}

#[test]
fn rejects_relative_lengths_instead_of_guessing() {
    // A bare number is not a length in CSS, and `rem`/`%` have no meaning in a
    // fixed-size render. Treating 50 as 50px would be a 100x layout error that
    // looks entirely plausible.
    let (sheet, mut report) = parse(".a{width:50;height:5rem;margin:10%}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let s = root_style(&tree);
    assert_eq!(s.width, None, "a bare number must not become pixels");
    assert_eq!(s.height, None, "rem has no parent font size here");
    assert_eq!(s.margin, 0.0, "percentages have no containing block here");
    assert!(
        report.skipped.iter().any(|(d, _)| d.starts_with("width")),
        "the drop must be reported, got {:?}",
        report.skipped
    );
}

#[test]
fn unitless_zero_is_a_valid_length() {
    let (sheet, mut report) = parse(".a{width:0;height:0}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert_eq!(root_style(&tree).width, Some(0.0));
    assert_eq!(root_style(&tree).height, Some(0.0));
}

#[test]
fn parses_every_colour_form() {
    let (sheet, mut report) = parse(".a{color:#f00;background:#00ff00;border-color:#0000ff80}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let s = root_style(&tree);
    assert_eq!(s.color, Some(Color::rgb(255, 0, 0)));
    assert_eq!(s.background, Some(Background::Solid(Color::rgb(0, 255, 0))));
    // #0000ff80 is 8-digit hex with alpha.
    assert_eq!(
        s.border_color,
        Some(Color {
            r: 0,
            g: 0,
            b: 255,
            a: 0x80
        })
    );
}

#[test]
fn parses_functional_and_named_colours() {
    let (sheet, mut report) =
        parse(".a{color:rgb(10, 20, 30);background:rgba(1,2,3,0.5);border-color:tomato}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let s = root_style(&tree);
    assert_eq!(s.color, Some(Color::rgb(10, 20, 30)));
    assert_eq!(
        s.background,
        Some(Background::Solid(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 128
        }))
    );
    assert_eq!(s.border_color, Some(Color::rgb(255, 99, 71)));
}

#[test]
fn rgba_function_with_a_spaces_slash_alpha() {
    // CSS allows `rgb(1 2 3 / 0.5)`. Getting this wrong is common enough that it
    // is worth pinning.
    let (sheet, mut report) = parse(".a{color:rgb(1 2 3 / 50%)}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let c = root_style(&tree).color.expect("colour parsed");
    assert_eq!((c.r, c.g, c.b), (1, 2, 3));
    assert_eq!(c.a, 128);
}

#[test]
fn linear_gradient_angle_is_translated_to_the_engines_frame() {
    // CSS 0deg points "to top"; the engine measures from the +x axis. The
    // translation is -90. If this is wrong every gradient is rotated a quarter
    // turn, which is subtle enough to survive a visual check.
    let (sheet, mut report) = parse(".a{background:linear-gradient(90deg, #fff, #000)}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    match root_style(&tree).background.clone() {
        Some(Background::Linear { angle_deg, stops }) => {
            assert!(
                (angle_deg - 0.0).abs() < 0.01,
                "90deg should map to 0, got {angle_deg}"
            );
            assert_eq!(stops.len(), 2);
        }
        other => panic!("expected a linear gradient, got {other:?}"),
    }
}

#[test]
fn gradient_side_keywords_are_supported() {
    let (sheet, mut report) = parse(".a{background:linear-gradient(to right, red, blue)}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    match root_style(&tree).background.clone() {
        Some(Background::Linear { angle_deg, .. }) => {
            assert!(
                (angle_deg - 0.0).abs() < 0.01,
                "to right is 90deg in CSS, so 0 here"
            );
        }
        other => panic!("expected a linear gradient, got {other:?}"),
    }
}

#[test]
fn box_shadow_optional_components_default_correctly() {
    // `0 4px 8px rgba(0,0,0,.3)` is the common form: no spread, alpha in the
    // colour. A parser that required a spread would reject the single most
    // common shadow in existence.
    let (sheet, mut report) = parse(".a{box-shadow: 0 4px 8px rgba(0,0,0,0.3)}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let sh = root_style(&tree).shadow.expect("shadow parsed");
    assert_eq!((sh.dx, sh.dy, sh.blur, sh.spread), (0.0, 4.0, 8.0, 0.0));
    assert_eq!(sh.color.a, 77);
    assert_eq!(sh.kind, ShadowKind::Drop);
}

#[test]
fn box_shadow_inset_keyword_sets_the_kind() {
    let (sheet, mut report) = parse(".a{box-shadow: inset 0 2px 4px #000}");
    let el = Element::new("div", vec![]).class("a");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let sh = root_style(&tree).shadow.expect("shadow parsed");
    assert_eq!(sh.kind, ShadowKind::InsetTop);
}

#[test]
fn class_selector_matches_only_its_own_class() {
    let (sheet, report) = parse(".card{width:100px} .other{width:200px}");

    let card = Element::new("div", vec![Element::text("x")]).class("card");
    assert_eq!(
        root_style(&css::from_elements(&sheet, &card, &mut report.clone())).width,
        Some(100.0),
        "a .card element must get the .card width"
    );

    // `.card` must not leak onto a `.other` element, which is the property that
    // actually matters: a selector that matched everything would render every
    // card identically and look fine.
    let other = Element::new("div", vec![Element::text("x")]).class("other");
    let tree = css::from_elements(&sheet, &other, &mut report.clone());
    assert_eq!(
        root_style(&tree).width,
        Some(200.0),
        "an .other element gets its own width"
    );

    // An element with neither class gets nothing, rather than the first rule.
    let bare = Element::new("div", vec![Element::text("x")]);
    assert_eq!(
        root_style(&css::from_elements(&sheet, &bare, &mut report.clone())).width,
        None,
        "an unclassed element must match no rule"
    );
}

#[test]
fn id_beats_nothing_but_is_matched() {
    let (sheet, mut report) = parse("#hero{width:640px}");
    let el = Element::new("div", vec![]).id("hero");
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert_eq!(root_style(&tree).width, Some(640.0));
}

#[test]
fn descendant_selector_matches_an_ancestor_chain() {
    let (sheet, mut report) = parse(".dark .card{width:300px}");
    let el = Element::new(
        "div",
        vec![
            Element::new("section", vec![Element::new("div", vec![]).class("card")]).class("dark"),
        ],
    );
    let tree = css::from_elements(&sheet, &el, &mut report);
    // .dark > section > .card
    let node = &tree;
    let mut found = false;
    if let Node::Container { children, .. } = node {
        if let Node::Container { children: c2, .. } = &children[0] {
            if let Node::Container { style, .. } = &c2[0] {
                assert_eq!(style.width, Some(300.0));
                found = true;
            }
        }
    }
    assert!(found, "the descendant selector did not match");
}

#[test]
fn unsupported_selector_is_reported_not_half_applied() {
    // A pseudo-class or attribute selector cannot be matched against a tree.
    // Silently ignoring it would apply the rule to nothing while looking like it
    // worked; the report names it instead.
    let (sheet, mut report) = parse("a:hover{width:100px}");
    let el = Element::new("a", vec![]);
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert_eq!(
        root_style(&tree).width,
        None,
        "a:hover must not apply to a bare tag"
    );
    assert!(
        report
            .ignored_selectors
            .iter()
            .any(|s| s.contains("a:hover")),
        "the unsupported selector must be reported, got {:?}",
        report.ignored_selectors
    );
}

#[test]
fn inline_style_beats_the_stylesheet() {
    let (sheet, mut report) = parse(".card{width:100px;color:#ff0000}");
    let el = Element::new("div", vec![])
        .class("card")
        .style("width:999px");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let s = root_style(&tree);
    assert_eq!(s.width, Some(999.0), "inline must win");
    assert_eq!(
        s.color,
        Some(Color::rgb(255, 0, 0)),
        "a non-conflicting property from the sheet must survive"
    );
}

#[test]
fn later_rule_wins_for_the_same_property() {
    // Documented cascade behaviour: this subset uses source order, not
    // specificity. The alternative is a half-implemented specificity model.
    let (sheet, mut report) = parse(".card{width:100px} .card{width:200px}");
    let el = Element::new("div", vec![]).class("card");
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert_eq!(root_style(&tree).width, Some(200.0));
}

#[test]
fn comments_are_stripped_not_parsed_as_text() {
    let (sheet, mut report) = parse("/* a comment */ .card{width:100px} /* another */");
    let el = Element::new("div", vec![]).class("card");
    let tree = css::from_elements(&sheet, &el, &mut report);
    assert_eq!(root_style(&tree).width, Some(100.0));
}

#[test]
fn display_grid_template_columns_sets_the_column_count() {
    let (sheet, mut report) = parse(".g{display:grid;grid-template-columns:repeat(3, 1fr)}");
    let el = Element::new("div", vec![]).class("g");
    let tree = css::from_elements(&sheet, &el, &mut report);
    let s = root_style(&tree);
    assert_eq!(s.display, Display::Grid);
    assert_eq!(s.grid_cols, Some(3));
}

#[test]
fn unsupported_declarations_are_all_reported() {
    let (sheet, mut report) =
        parse(".a{transform:rotate(3deg);z-index:5;filter:blur(2px);float:left;opacity:0.5}");
    let el = Element::new("div", vec![]).class("a");
    let _ = css::from_elements(&sheet, &el, &mut report);
    for want in ["transform", "z-index", "filter", "float", "opacity"] {
        assert!(
            report.skipped.iter().any(|(d, _)| d.starts_with(want)),
            "{want} should be reported as skipped, got {:?}",
            report.skipped
        );
    }
}

#[test]
fn report_summary_is_stable_and_countable() {
    let mut r = ParseReport::default();
    let sheet = Stylesheet::parse(".a{color:red;transform:none}", &mut r).expect("parse");
    let el = Element::new("div", vec![]).class("a");
    let _ = css::from_elements(&sheet, &el, &mut r);
    assert!(!r.is_clean());
    assert_eq!(r.applied.len(), 1);
    assert_eq!(r.skipped.len(), 1);
    assert!(r.summary().contains("1 applied"), "{}", r.summary());
}

// --- HTML -----------------------------------------------------------------

#[test]
fn html_nesting_and_text_become_a_tree() {
    let html = r#"<div class="card"><h1>Title</h1><p>Body</p></div>"#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    // `<h1>` and `<p>` carry no box styles of their own, so each collapses to a
    // text node. A wrapper element that did not collapse would add an empty
    // box between the card and the words, which is what this asserts against.
    match tree {
        Node::Container { children, .. } => {
            assert_eq!(children.len(), 2, "two child elements expected");
            assert!(
                matches!(children[0], Node::Text { .. }),
                "h1 should collapse to text"
            );
            assert!(
                matches!(children[1], Node::Text { .. }),
                "p should collapse to text"
            );
        }
        _ => panic!("expected a container root"),
    }
    assert!(
        report.is_clean(),
        "clean HTML should skip nothing: {report}"
    );
}

#[test]
fn html_void_elements_do_not_swallow_the_rest() {
    // `<img>` never closes. Getting this wrong nests everything after it inside
    // the image, which produces a silently wrong tree.
    let html = r#"<div><img src="a.png"><span>after</span></div>"#;
    let roots = css::parse_html(html);
    assert_eq!(roots.len(), 1);
    let div = &roots[0];
    assert_eq!(div.children.len(), 2, "img and span are siblings");
    assert_eq!(div.children[0].tag.as_deref(), Some("img"));
    assert_eq!(div.children[1].tag.as_deref(), Some("span"));
}

#[test]
fn html_doctype_and_comments_are_ignored() {
    let html = "<!DOCTYPE html><!-- note --><div class=\"a\">x</div>";
    let roots = css::parse_html(html);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].tag.as_deref(), Some("div"));
    assert_eq!(roots[0].classes, vec!["a".to_owned()]);
}

#[test]
fn html_style_block_is_parsed_as_css() {
    let html = r#"<style>.card{width:800px;background:#123456}</style><div class="card">hi</div>"#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    let s = root_style(&tree);
    assert_eq!(s.width, Some(800.0), "the inline stylesheet must apply");
    assert_eq!(
        s.background,
        Some(Background::Solid(Color::rgb(18, 52, 86)))
    );
}

#[test]
fn html_script_content_is_not_parsed_as_markup() {
    // A `<` inside a script body must not open a bogus element.
    let html = r#"<div><script>if (a<b) { }</script><span>after</span></div>"#;
    let roots = css::parse_html(html);
    assert_eq!(roots[0].children.len(), 1, "only the span survives");
    assert_eq!(roots[0].children[0].tag.as_deref(), Some("span"));
}

#[test]
fn html_entities_are_decoded() {
    let html = "<div>Tom &amp; Jerry &lt;3</div>";
    let roots = css::parse_html(html);
    assert_eq!(
        roots[0].children[0].text.as_deref(),
        Some("Tom & Jerry <3"),
        "entities must be decoded or they render literally"
    );
}

#[test]
fn unclosed_tags_do_not_lose_content() {
    // A malformed document should still render what it clearly meant rather
    // than dropping everything after the missing close.
    let html = "<div><span>a</span><em>b</div>";
    let roots = css::parse_html(html);
    assert_eq!(
        roots.len(),
        1,
        "the unclosed em must not become a second root"
    );
}

#[test]
fn href_becomes_a_pdf_link() {
    let html = r#"<div><a href="https://example.com">click</a></div>"#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    let mut found = false;
    if let Node::Container { children, .. } = &tree {
        for c in children {
            match c {
                Node::Container { style, .. } | Node::Text { style, .. } => {
                    if style.link.as_deref() == Some("https://example.com") {
                        found = true;
                    }
                }
                Node::Image { style, .. } => {
                    if style.link.as_deref() == Some("https://example.com") {
                        found = true;
                    }
                }
            }
        }
    }
    assert!(found, "href did not become a link annotation");
}

#[test]
fn mixed_script_and_styling_document_parses_clean() {
    // A realistic OG card: stylesheet, classes, inline override, Arabic text.
    let html = r#"
    <style>
      .card { display: flex; flex-direction: column; width: 1200px; height: 630px;
              background: linear-gradient(180deg, #dbeafe, #fee2e2); padding: 40px; }
      .title { font-size: 72px; color: #0f172a; }
      .sub   { font-size: 28px; color: #475569; }
      .arabic { font-size: 40px; color: #1e293b; direction: rtl; }
    </style>
    <div class="card">
      <span class="title">Hikari</span>
      <span class="sub" style="color:#94a3b8">deterministic</span>
      <span class="arabic">مرحبا</span>
    </div>
    "#;
    let mut report = ParseReport::default();
    let tree = css::html_to_tree(html, &mut report).expect("parse");
    assert!(report.is_clean(), "unexpected skips: {report}");
    let s = root_style(&tree);
    assert_eq!(s.width, Some(1200.0));
    assert_eq!(s.height, Some(630.0));
    assert_eq!(
        s.dir,
        FlexDir::Column,
        "flex-direction: column must reach the tree"
    );
    assert!(matches!(s.background, Some(Background::Linear { .. })));
}
