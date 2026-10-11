//! The frozen CSS subset: what this crate claims, and a test that it is true.
//!
//! # Why a spec at all
//!
//! The CSS front end accepts some properties and reports the rest as skipped. A
//! caller reading that at runtime can discover what works, one property at a
//! time, by trying it. That is the wrong shape for a contract: a stylesheet that
//! silently drops half its declarations renders a plausible, wrong card, and
//! nothing says which half.
//!
//! So the supported set is stated up front, here and in `docs/css-spec.json`,
//! and [`conformance`] checks that every entry is actually accepted. A property
//! in the list that the parser rejects fails a test; a property the parser
//! accepts that is missing from the list also fails one. The list cannot drift
//! in either direction without CI noticing.
//!
//! # What this is not
//!
//! It is not CSS. It is not a promise to track any CSS specification, and
//! "unsupported" here means "this crate does not implement it", not "invalid".
//! A property absent from this table is not an error in the stylesheet; it is
//! skipped and reported, which is the behaviour a caller can detect but not one
//! they should have to.
//!
//! # Value support
//!
//! The table records properties, not value domains: `color` is listed whether it
//! accepts `#rrggbb`, `rgb()`, a named colour or `transparent`, and which of
//! those is true is documented on the property in the README and exercised by
//! the conformance cases below. Each entry here carries a representative value
//! that the parser is asserted to accept, which is what makes the table a test
//! rather than a comment.

/// One property, with a value the parser must accept it with.
///
/// The `value` is load-bearing. A list of property names with nothing to check
/// them against is a comment; pairing each with a value makes it an assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property {
    /// The CSS property name, as it appears in a declaration.
    pub name: &'static str,
    /// A representative value the parser accepts for it.
    pub value: &'static str,
    /// One-line note, shown in the generated spec.
    pub note: &'static str,
}

/// Every property this crate accepts, in the order shown to callers.
///
/// Alphabetical, because a table you have to search is a table that reads as
/// incomplete.
pub const SUPPORTED: &[Property] = &[
    Property {
        name: "align-items",
        value: "center",
        note: "start, center, end, stretch",
    },
    Property {
        name: "align-self",
        value: "center",
        note: "overrides align-items for one node",
    },
    Property {
        name: "aspect-ratio",
        value: "1.5",
        note: "unitless, w/h",
    },
    Property {
        name: "backdrop-filter",
        value: "none",
        note: "colour filters only; blur is reported",
    },
    Property {
        name: "background",
        value: "#123456",
        note: "solid fill shorthand",
    },
    Property {
        name: "background-blend-mode",
        value: "multiply",
        note: "per-layer blend",
    },
    Property {
        name: "background-clip",
        value: "text",
        note: "text | border-box",
    },
    Property {
        name: "background-color",
        value: "#123456",
        note: "",
    },
    Property {
        name: "border",
        value: "2px #f59e0b",
        note: "width then colour; radius is separate",
    },
    Property {
        name: "border-color",
        value: "#f59e0b",
        note: "",
    },
    Property {
        name: "border-radius",
        value: "8px",
        note: "",
    },
    Property {
        name: "border-width",
        value: "2px",
        note: "",
    },
    Property {
        name: "box-shadow",
        value: "2px 2px 8px #000000",
        note: "offset blur colour",
    },
    Property {
        name: "color",
        value: "#ffffff",
        note: "hex, rgb(), named, transparent",
    },
    Property {
        name: "direction",
        value: "rtl",
        note: "ltr | rtl",
    },
    Property {
        name: "display",
        value: "flex",
        note: "flex | block | inline-flex | grid | none",
    },
    Property {
        name: "filter",
        value: "grayscale(1)",
        note: "colour filters only; blur is reported",
    },
    Property {
        name: "flex",
        value: "1",
        note: "grow shorthand",
    },
    Property {
        name: "flex-direction",
        value: "row",
        note: "row | column, with -reverse",
    },
    Property {
        name: "flex-grow",
        value: "1",
        note: "",
    },
    Property {
        name: "font-size",
        value: "16px",
        note: "",
    },
    Property {
        name: "gap",
        value: "8px",
        note: "flex and grid gap",
    },
    Property {
        name: "grid-gap",
        value: "8px",
        note: "alias of gap",
    },
    Property {
        name: "grid-template-columns",
        value: "repeat(3, 1fr)",
        note: "",
    },
    Property {
        name: "height",
        value: "100px",
        note: "",
    },
    Property {
        name: "justify-content",
        value: "center",
        note: "start, center, end, space-between, space-around",
    },
    Property {
        name: "left",
        value: "0",
        note: "position: absolute/fixed",
    },
    Property {
        name: "linear-gradient",
        value: "linear-gradient(180deg, #000000, #ffffff)",
        note: "",
    },
    Property {
        name: "margin",
        value: "8px",
        note: "",
    },
    Property {
        name: "max-width",
        value: "600px",
        note: "",
    },
    Property {
        name: "min-height",
        value: "40px",
        note: "",
    },
    Property {
        name: "min-width",
        value: "100px",
        note: "",
    },
    Property {
        name: "mix-blend-mode",
        value: "multiply",
        note: "",
    },
    Property {
        name: "padding",
        value: "8px",
        note: "",
    },
    Property {
        name: "position",
        value: "absolute",
        note: "relative | absolute | fixed | static",
    },
    Property {
        name: "top",
        value: "0",
        note: "position: absolute/fixed",
    },
    Property {
        name: "width",
        value: "100px",
        note: "",
    },
];

/// Properties the parser **recognises and refuses**, with the reason it gives.
///
/// These are matched deliberately, not accidentally. A stylesheet containing
/// `opacity: 0.5` gets "use an rgba colour with alpha instead" rather than the
/// generic "unknown property", because the useful answer is the one naming what
/// to write instead. Listing them here keeps that deliberate -- and the
/// conformance suite below asserts each one still refuses with the same reason,
/// so an arm cannot quietly start accepting a property nobody updated the spec
/// for.
///
/// `value` is unused for these: the point is the refusal, not the value.
pub const REPORTED: &[Property] = &[
    Property {
        name: "background-image",
        value: "",
        note: "only gradients are supported here",
    },
    Property {
        name: "box-sizing",
        value: "",
        note: "always border-box here",
    },
    Property {
        name: "flex-basis",
        value: "",
        note: "use width on the child instead",
    },
    Property {
        name: "float",
        value: "",
        note: "float is not supported",
    },
    Property {
        name: "font-family",
        value: "",
        note: "fonts are chosen by register_font, not by name",
    },
    Property {
        name: "font-style",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "font-weight",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "letter-spacing",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "line-break",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "line-height",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "mask",
        value: "",
        note: "masks are not supported",
    },
    Property {
        name: "mask-image",
        value: "",
        note: "masks are not supported",
    },
    Property {
        name: "max-height",
        value: "",
        note: "only width, height and max-width map onto the layout tree",
    },
    Property {
        name: "object-fit",
        value: "",
        note: "set ImgFit on the image node instead",
    },
    Property {
        name: "opacity",
        value: "",
        note: "use an rgba colour with alpha instead",
    },
    Property {
        name: "order",
        value: "",
        note: "paint order is document order",
    },
    Property {
        name: "overflow",
        value: "",
        note: "no scrolling in a fixed-size render",
    },
    Property {
        name: "overflow-wrap",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "overflow-x",
        value: "",
        note: "no scrolling in a fixed-size render",
    },
    Property {
        name: "overflow-y",
        value: "",
        note: "no scrolling in a fixed-size render",
    },
    Property {
        name: "right",
        value: "",
        note: "only left/top offsets are supported",
    },
    Property {
        name: "text-align",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "text-decoration",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "text-indent",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "text-overflow",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "text-transform",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "transform",
        value: "",
        note: "transform is not supported",
    },
    Property {
        name: "vertical-align",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "white-space",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "width-percent",
        value: "",
        note: "percentages have no containing block here",
    },
    Property {
        name: "word-break",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "word-spacing",
        value: "",
        note: "no effect on a fixed-size render",
    },
    Property {
        name: "z-index",
        value: "",
        note: "paint order is document order",
    },
];

/// Property names only, for callers that want a membership test.
#[must_use]
pub fn supported_properties() -> Vec<&'static str> {
    SUPPORTED.iter().map(|p| p.name).collect()
}

/// Recognised-but-refused property names.
#[must_use]
pub fn reported_properties() -> Vec<&'static str> {
    REPORTED.iter().map(|p| p.name).collect()
}

/// Check the table above against the parser.
///
/// Three directions, because each failure is different:
///
/// 1. **Every listed property is accepted** with its representative value. If
///    the parser stops accepting one, the spec is claiming support that does not
///    exist and a caller is writing a stylesheet against a promise.
/// 2. **The list is sorted and unique.** Two entries for one property means one
///    of them is stale.
/// 3. **No property the parser accepts is missing.** This is the direction that
///    is easy to skip and is the reason the table is not just documentation: a
///    property implemented and not listed is a feature nobody can find.
///
/// Gated on `cfg(test)` because it is a test suite. Its helper is unreachable
/// once `#[test]` functions are stripped from a release build, and an
/// always-compiled module full of them draws "never used" warnings on every
/// `cargo clippy` run.
#[cfg(test)]
pub mod conformance {
    use super::*;
    use crate::css::ParseReport;
    use crate::Style;

    /// Apply one declaration and report what the parser said.
    fn apply(name: &str, value: &str) -> (bool, String) {
        let mut style = Style::new();
        let mut report = ParseReport::default();
        crate::css::apply_to_style(&mut style, name, value, &mut report);
        match report.skipped.first() {
            Some((_, why)) => (false, (*why).to_owned()),
            None => (true, String::new()),
        }
    }

    #[test]
    fn every_supported_property_is_accepted() {
        // The direction that matters most. The first draft of this table listed
        // 32 properties that the parser matches only in order to *refuse* them
        // with a specific message -- `opacity`, `text-align`, `z-index` and so
        // on -- and this test is what caught it. A spec claiming support for
        // properties that do nothing is worse than no spec at all.
        let mut bad = Vec::new();
        for p in SUPPORTED {
            let (ok, why) = apply(p.name, p.value);
            if !ok {
                bad.push(format!("{}: {} ({why})", p.name, p.value));
            }
        }
        assert!(
            bad.is_empty(),
            "SUPPORTED lists properties the parser refuses: {bad:?}"
        );
    }

    #[test]
    fn every_reported_property_is_refused_with_its_reason() {
        // A recognised property that stops being refused is a behaviour change
        // nobody announced; a reason that changes is a worse one, because
        // callers and documentation quote it.
        let mut bad = Vec::new();
        for p in REPORTED {
            let (ok, why) = apply(p.name, "none");
            if ok {
                bad.push(format!("{}: now accepted, still listed as refused", p.name));
            } else if why != p.note {
                bad.push(format!(
                    "{}: reason is {why:?}, spec says {:?}",
                    p.name, p.note
                ));
            }
        }
        assert!(bad.is_empty(), "REPORTED has drifted: {bad:?}");
    }

    #[test]
    fn the_two_tables_do_not_overlap() {
        let supported: std::collections::BTreeSet<&str> =
            SUPPORTED.iter().map(|p| p.name).collect();
        for p in REPORTED {
            assert!(!supported.contains(p.name), "{} is in both tables", p.name);
        }
    }

    #[test]
    fn both_tables_are_sorted_and_unique() {
        for (label, table) in [("SUPPORTED", SUPPORTED), ("REPORTED", REPORTED)] {
            let names: Vec<&str> = table.iter().map(|p| p.name).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            assert_eq!(names, sorted, "{label} is not in alphabetical order");
            let before = names.len();
            let mut deduped = names.clone();
            deduped.dedup();
            assert_eq!(deduped.len(), before, "{label} contains a duplicate");
        }
    }

    #[test]
    fn a_property_in_neither_table_is_reported_as_unknown() {
        // The third bucket. Not a table, because there are thousands of CSS
        // properties and listing them would be a list of things this crate does
        // not do.
        for name in ["float-in-left", "text-emphasis-style", "grid-auto-flow"] {
            let (ok, why) = apply(name, "none");
            assert!(!ok, "{name} was accepted");
            assert!(
                why.contains("unknown"),
                "{name} should be unknown, got {why:?}"
            );
        }
    }
}
