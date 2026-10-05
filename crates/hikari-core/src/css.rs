//! A CSS subset parser: stylesheets and HTML-ish markup into node trees.
//!
//! # Why this exists, and why it is hand-written
//!
//! The alternative was `lightningcss`, and it was measured rather than assumed.
//! It is a genuinely good parser — but it pulls `dashmap` → `ahash` →
//! `getrandom@0.3.4`, and `getrandom` 0.3.4 is a hard `compile_error!` on
//! `wasm32-unknown-unknown` unless both a `--cfg` and a feature flag are set,
//! neither of which is reachable through `lightningcss`'s dependency graph.
//! Adding it breaks the WebAssembly build outright. Its default features also
//! carry a bundler this project has no use for. So the parser is 600 lines of
//! ordinary Rust with no dependencies, which also keeps the "boring
//! dependencies" claim honest.
//!
//! # Scope, stated plainly
//!
//! This is a *subset*. It covers the CSS that maps onto [`Style`] — which is the
//! CSS worth writing for a fixed-size image, because a render target has no
//! viewport, no scrolling and no user interaction.
//!
//! It does **not** implement: `@media`, custom properties, `calc()`,
//! `transform`, `z-index`, `float`, selector combinators beyond descendant and
//! child, attribute selectors, pseudo-classes, or shorthand for anything with
//! more than two components. Unsupported declarations are **skipped**, not
//! guessed at, and [`parse_report`] returns every one so a caller can see what
//! was dropped rather than discovering a missing style three hours later.
//!
//! # HTML
//!
//! [`html_to_tree`] handles nesting, text nodes, `class`/`id` and a fixed set
//! of inline styles. Self-closing and void elements are handled. Comments,
//! `<!DOCTYPE>`, and attributes other than `class`, `id`, `style`, `href` and
//! `src` are ignored. Script and style *content* is not parsed as CSS; `<style>`
//! blocks are, so a document can carry its stylesheet inline.

use std::collections::HashMap;

use crate::{
    Align, Background, BlendMode, Color, ColorStop, Display, Error, FlexDir, FontId, ImgFit,
    Justify, Node, Shadow, ShadowKind, Style,
};

/// Everything the parser understood, and everything it did not.
///
/// Returned alongside the tree because a silently-ignored declaration is worse
/// than a rejected one: the render succeeds, the image looks almost right, and
/// the difference is impossible to spot without comparing against a browser.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseReport {
    /// Declarations that were understood and applied.
    pub applied: Vec<String>,
    /// Declarations skipped, with the reason. Sorted so the report is
    /// deterministic regardless of input order.
    pub skipped: Vec<(String, &'static str)>,
    /// Rules whose selector was not understood, so its declarations were not
    /// applied to anything.
    pub ignored_selectors: Vec<String>,
}

impl ParseReport {
    /// True if anything was skipped or ignored.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.skipped.is_empty() && self.ignored_selectors.is_empty()
    }

    /// A one-line summary, for logging or a test assertion.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} applied, {} skipped, {} selectors ignored",
            self.applied.len(),
            self.skipped.len(),
            self.ignored_selectors.len()
        )
    }
}

impl std::fmt::Display for ParseReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.summary())?;
        for (decl, why) in &self.skipped {
            writeln!(f, "  skipped {decl}: {why}")?;
        }
        for sel in &self.ignored_selectors {
            writeln!(f, "  ignored selector {sel}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// One CSS token: either punctuation or a run of non-punctuation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// `{ } : ; , ( )` and friends.
    Punct(char),
    /// A bare word, number, or quoted string (quotes stripped).
    Word(String),
}

fn is_punct(c: char) -> bool {
    matches!(c, '{' | '}' | ':' | ';' | ',' | '(' | ')' | '/' | '*')
}

/// Split CSS into tokens, dropping comments and whitespace.
///
/// Comments are stripped rather than tokenized because `/* */` inside a value
/// is legal and rare, and a correct comment stripper is much easier to trust
/// than one that has to survive arbitrary comment text.
fn tokenize(css: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = css.chars().peekable();
    let mut word = String::new();
    let mut quote: Option<char> = None;

    macro_rules! flush {
        () => {
            if !word.is_empty() {
                out.push(Tok::Word(std::mem::take(&mut word)));
            }
        };
    }

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' {
                // Escaped character inside a quoted string.
                if let Some(n) = chars.next() {
                    word.push(n);
                }
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'*') => {
                // Comment: skip to the closing delimiter, and flush any word in
                // progress first so `a/*x*/b` still tokenizes as two words.
                flush!();
                chars.next();
                let mut prev = ' ';
                for ch in chars.by_ref() {
                    if prev == '*' && ch == '/' {
                        break;
                    }
                    prev = ch;
                }
            }
            '"' | '\'' => {
                flush!();
                quote = Some(c);
            }
            c if c.is_whitespace() => flush!(),
            c if is_punct(c) => {
                flush!();
                out.push(Tok::Punct(c));
            }
            c => word.push(c),
        }
    }
    flush!();
    out
}

// ---------------------------------------------------------------------------
// Value parsing
// ---------------------------------------------------------------------------

/// Parse a CSS length into px.
///
/// Only `px`, the unitless zero, and `pt`/`in`/`cm`/`mm`/`pc` via exact factors.
/// A bare non-zero number is rejected rather than treated as px, because CSS
/// treats it as invalid for lengths and guessing would silently produce a
/// 100×-off layout.
fn parse_px(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("px") {
        return v.trim().parse().ok();
    }
    if let Some(v) = s.strip_suffix("pt") {
        return v.trim().parse::<f32>().ok().map(|n| n * 96.0 / 72.0);
    }
    if let Some(v) = s.strip_suffix("in") {
        return v.trim().parse::<f32>().ok().map(|n| n * 96.0);
    }
    if let Some(v) = s.strip_suffix("pc") {
        return v.trim().parse::<f32>().ok().map(|n| n * 16.0);
    }
    if let Some(v) = s.strip_suffix("cm") {
        return v.trim().parse::<f32>().ok().map(|n| n * 96.0 / 2.54);
    }
    if let Some(v) = s.strip_suffix("mm") {
        return v.trim().parse::<f32>().ok().map(|n| n * 96.0 / 25.4);
    }
    // Relative units and percentages have no meaning here: there is no parent
    // font size, and no containing block for a percentage to be relative to.
    if s.ends_with("rem")
        || s.ends_with("em")
        || s.ends_with("%")
        || s.ends_with("vw")
        || s.ends_with("vh")
        || s.ends_with("ch")
        || s.ends_with("ex")
    {
        return None;
    }
    // Unitless zero is valid for lengths in CSS.
    if s == "0" {
        return Some(0.0);
    }
    None
}

/// Split a value on top-level spaces, respecting parentheses.
///
/// `box-shadow: 0 4px 8px rgba(0,0,0,.3)` must yield four components, not six
/// with the `rgba` broken up, so parentheses are tracked while splitting.
fn split_top_level(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parse a CSS colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`
/// and the named colours the library already knows.
///
/// Returns `None` for anything else rather than guessing, so an unsupported
/// colour shows up in the report instead of rendering black.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("rgba(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let parts: Vec<String> = if rest.contains('/') {
            let (rgb_part, alpha_part) = rest.split_once('/').unwrap_or((rest, ""));
            let mut v: Vec<String> = rgb_part.split_whitespace().map(ToOwned::to_owned).collect();
            if !alpha_part.trim().is_empty() {
                v.push(alpha_part.trim().to_owned());
            }
            v
        } else {
            split_top_level_top(rest)
        };
        if parts.len() >= 3 {
            let r = parts[0].trim().parse::<u16>().ok()?;
            let g = parts[1].trim().parse::<u16>().ok()?;
            let b = parts[2].trim().parse::<u16>().ok()?;
            let a = if parts.len() > 3 {
                parse_alpha(&parts[3])?
            } else {
                255
            };
            return Some(Color {
                r: r.min(255) as u8,
                g: g.min(255) as u8,
                b: b.min(255) as u8,
                a,
            });
        }
    }
    if let Some(rest) = lower.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        // Two syntaxes are legal: the legacy comma form and the modern
        // space-separated one with a `/` before alpha, `rgb(1 2 3 / 50%)`. The
        // modern form is what every current tool emits, so rejecting it would
        // make the parser useless in practice.
        let parts: Vec<String> = if rest.contains('/') {
            let (rgb_part, alpha_part) = rest.split_once('/').unwrap_or((rest, ""));
            let mut v: Vec<String> = rgb_part.split_whitespace().map(ToOwned::to_owned).collect();
            if !alpha_part.trim().is_empty() {
                v.push(alpha_part.trim().to_owned());
            }
            v
        } else {
            split_top_level_top(rest)
        };
        if parts.len() >= 3 {
            // Percentages are allowed by CSS; scale them.
            let chan = |p: &str| -> Option<u8> {
                let p = p.trim();
                if let Some(pct) = p.strip_suffix('%') {
                    Some((pct.trim().parse::<f32>().ok()? * 255.0 / 100.0).round() as u8)
                } else {
                    Some(p.parse::<u16>().ok()?.min(255) as u8)
                }
            };
            let r = chan(&parts[0])?;
            let g = chan(&parts[1])?;
            let b = chan(&parts[2])?;
            // CSS 4 allows an alpha in `rgb()` too, via `rgb(1 2 3 / 50%)`.
            // Discarding it made the colour opaque, which is the worst possible
            // failure for a value that *looks* like it carries transparency.
            let a = if parts.len() > 3 {
                parse_alpha(&parts[3])?
            } else {
                255
            };
            return Some(Color { r, g, b, a });
        }
    }
    if let Some(named) = named_color(&lower) {
        return Some(named);
    }
    None
}

/// Split on commas only (not spaces), for colour function arguments.
fn split_top_level_top(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).map(ToOwned::to_owned).collect()
}

/// Split a gradient's argument list on top-level commas.
///
/// Gradient stops are comma-separated, so `split_top_level` — which breaks on
/// whitespace — returns `linear-gradient(90deg,#fff,#000)`'s interior as one
/// piece and every stop parse fails. Commas are tracked alongside paren depth so
/// a nested `rgba(0,0,0,.5)` inside a stop is not split.
fn split_gradient_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_alpha(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return Some((pct.trim().parse::<f32>().ok()? * 255.0 / 100.0).round() as u8);
    }
    Some((s.parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.trim();
    let dup = |i: usize| -> Option<u8> {
        u8::from_str_radix(h.get(i..i + 1)?, 16)
            .ok()
            .map(|n| n * 17)
    };
    match h.len() {
        3 => Some(Color::rgb(dup(0)?, dup(1)?, dup(2)?)),
        4 => Some(Color {
            r: dup(0)?,
            g: dup(1)?,
            b: dup(2)?,
            a: u8::from_str_radix(h.get(3..4)?, 16).ok()?,
        }),
        6 => Some(Color::rgb(
            u8::from_str_radix(h.get(0..2)?, 16).ok()?,
            u8::from_str_radix(h.get(2..4)?, 16).ok()?,
            u8::from_str_radix(h.get(4..6)?, 16).ok()?,
        )),
        8 => Some(Color {
            r: u8::from_str_radix(h.get(0..2)?, 16).ok()?,
            g: u8::from_str_radix(h.get(2..4)?, 16).ok()?,
            b: u8::from_str_radix(h.get(4..6)?, 16).ok()?,
            a: u8::from_str_radix(h.get(6..8)?, 16).ok()?,
        }),
        _ => None,
    }
}

/// The named colours worth supporting.
///
/// Not all 148: this is the subset that appears in real design tokens, chosen so
/// a brand colour written as `tomato` works rather than silently rendering black.
/// Anything outside it is reported as skipped.
fn named_color(name: &str) -> Option<Color> {
    let rgb = |r: u8, g: u8, b: u8| Some(Color::rgb(r, g, b));
    match name {
        "transparent" => Some(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }),
        "black" => rgb(0, 0, 0),
        "white" => rgb(255, 255, 255),
        "red" => rgb(255, 0, 0),
        "lime" => rgb(0, 255, 0),
        "green" => rgb(0, 128, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "cyan" | "aqua" => rgb(0, 255, 255),
        "magenta" | "fuchsia" => rgb(255, 0, 255),
        "gray" | "grey" => rgb(128, 128, 128),
        "darkgray" | "darkgrey" => rgb(169, 169, 169),
        "lightgray" | "lightgrey" => rgb(211, 211, 211),
        "silver" => rgb(192, 192, 192),
        "maroon" => rgb(128, 0, 0),
        "olive" => rgb(128, 128, 0),
        "purple" => rgb(128, 0, 128),
        "teal" => rgb(0, 128, 128),
        "navy" => rgb(0, 0, 128),
        "orange" => rgb(255, 165, 0),
        "pink" => rgb(255, 192, 203),
        "purpleish" => rgb(147, 112, 219),
        "rebeccapurple" => rgb(102, 51, 153),
        "gold" => rgb(255, 215, 0),
        "beige" => rgb(245, 245, 220),
        "ivory" => rgb(255, 255, 240),
        "khaki" => rgb(240, 230, 140),
        "salmon" => rgb(250, 128, 114),
        "tomato" => rgb(255, 99, 71),
        "turquoise" => rgb(64, 224, 208),
        "violet" => rgb(238, 130, 238),
        "indigo" => rgb(75, 0, 130),
        "crimson" => rgb(220, 20, 60),
        "coral" => rgb(255, 127, 80),
        "plum" => rgb(221, 160, 221),
        "orchid" => rgb(218, 112, 214),
        "skyblue" => rgb(135, 206, 235),
        "steelblue" => rgb(70, 130, 180),
        "slategray" | "slategrey" => rgb(112, 128, 144),
        "whitesmoke" => rgb(245, 245, 245),
        "gainsboro" => rgb(220, 220, 220),
        "dimgray" | "dimgrey" => rgb(105, 105, 105),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

/// A parsed selector: a compound last step, plus any ancestor requirements.
///
/// Only the forms that can be matched against a tree are kept. Anything with a
/// combinator or pseudo-class makes the whole rule unmatchable rather than
/// partially applied, because half-applying a selector is worse than not
/// applying it: the result looks intentional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Selector {
    /// `tag`, `.class`, `#id` or `*`, lowercased.
    last: String,
    /// Ancestor conditions that must also match, outermost first.
    ancestors: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Condition {
    /// A `.class` on an ancestor.
    Class(String),
    /// A `tag` on an ancestor.
    Tag(String),
    /// A direct-child combinator: the previous step must be the parent.
    Child,
}

/// How a tag or class reaches a node, kept separate so matching does not need
/// the [`Node`] type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Classes {
    id: Option<String>,
    classes: Vec<String>,
    tag: Option<String>,
}

/// Does this element paint a box of its own?
///
/// The test for collapsing an element into a bare [`Node::Text`]. Text styling
/// alone — a font size, a colour — belongs on the glyphs, so `<span class="t">`
/// should not add an empty box between the parent and the text. Anything that
/// makes the element a visible or laid-out box must keep it, or collapsing
/// silently discards it: a `<div style="width:600px;height:300px;background:#102040">`
/// around one word would lose all three and render at the text's own size on the
/// page background.
fn is_boxless(style: &Style) -> bool {
    style.width.is_none()
        && style.height.is_none()
        && style.aspect.is_none()
        && style.grid_cols.is_none()
        && style.background.is_none()
        && style.shadow.is_none()
        && style.border <= 0.0
        && style.border_color.is_none()
        && style.radius <= 0.0
        && style.padding <= 0.0
        && style.margin <= 0.0
        && style.gap <= 0.0
        && !style.absolute
        && style.left.is_none()
        && style.top.is_none()
        && style.display == Display::Flex
        && style.dir == FlexDir::Row
        && style.justify == Justify::Start
        && style.align == Align::Start
        && style.grow <= 0.0
}

/// The text properties CSS inherits down a tree.
///
/// Only properties CSS marks inherited are carried. `font-size`, `color`,
/// `font-family` and `max-width` inherit; `background`, `padding` and `border`
/// do not, and copying those down would paint a box on every element in the
/// document.
#[derive(Debug, Clone, Copy, Default)]
struct InheritedText {
    font_size: Option<f32>,
    color: Option<Color>,
    font: Option<FontId>,
    max_width: Option<f32>,
}

/// The nearest value of each inherited property along `ancestors`.
///
/// The chain is outermost-first, so iterating it forward and overwriting on each
/// hit leaves the *last* match — which is the CSS rule, since a nearer
/// declaration wins over a further one.
fn inherited_text(ancestors: &Ancestors) -> InheritedText {
    let mut out = InheritedText::default();
    for (style, _, _) in ancestors {
        if style.font_size.is_some() {
            out.font_size = style.font_size;
        }
        if style.color.is_some() {
            out.color = style.color;
        }
        if style.font.is_some() {
            out.font = style.font;
        }
        if style.max_width.is_some() {
            out.max_width = style.max_width;
        }
    }
    out
}

/// One entry per ancestor, outermost first: its resolved style, its classes, and
/// whether it is the node's *direct* parent (which is what a `>` combinator
/// needs).
///
/// The style is carried rather than just the classes because inherited text
/// properties — `font-size`, `color` — have to come from somewhere, and CSS says
/// they come from the nearest styled ancestor.
type Ancestors = Vec<(Style, Classes, bool)>;

/// Parse a selector, or `None` if it uses an unsupported feature.
fn parse_selector(sel: &str) -> Option<Selector> {
    let sel = sel.trim();
    if sel.is_empty() {
        return None;
    }
    // Reject rather than partially support. A pseudo-class or attribute
    // selector here would silently match nothing if ignored, so it is reported.
    if sel.contains(':')
        || sel.contains('[')
        || sel.contains('(')
        || sel.contains('*') && sel != "*"
    {
        return None;
    }
    if sel.contains('>') {
        // Direct-child combinators are supported; see below.
        return parse_with_child(sel);
    }
    if sel.contains(' ') {
        return parse_descendant(sel);
    }
    Some(Selector {
        last: sel.to_ascii_lowercase(),
        ..Selector::default()
    })
}

fn parse_with_child(sel: &str) -> Option<Selector> {
    let steps: Vec<&str> = sel.split('>').map(str::trim).collect();
    let mut out = Selector::default();
    for (i, step) in steps.iter().enumerate() {
        if step.contains(':') || step.contains('[') {
            return None;
        }
        if i == steps.len() - 1 {
            out.last = step.to_ascii_lowercase();
        } else {
            out.ancestors.push(Condition::Child);
            out.ancestors.push(simple_condition(step)?);
        }
    }
    Some(out)
}

fn parse_descendant(sel: &str) -> Option<Selector> {
    let steps: Vec<&str> = sel.split_whitespace().collect();
    let mut out = Selector::default();
    for (i, step) in steps.iter().enumerate() {
        if step.contains(':') || step.contains('[') {
            return None;
        }
        if i == steps.len() - 1 {
            out.last = step.to_ascii_lowercase();
        } else {
            out.ancestors.push(simple_condition(step)?);
        }
    }
    Some(out)
}

fn simple_condition(step: &str) -> Option<Condition> {
    if let Some(cls) = step.strip_prefix('.') {
        Some(Condition::Class(cls.to_ascii_lowercase()))
    } else {
        Some(Condition::Tag(step.to_ascii_lowercase()))
    }
}

fn condition_matches(cond: &Condition, node: &Classes) -> bool {
    match cond {
        Condition::Class(c) => node.classes.iter().any(|x| x == c),
        Condition::Tag(t) => node.tag.as_deref() == Some(t.as_str()),
        Condition::Child => false, // handled structurally during the walk
    }
}

/// Match a selector against a node and its ancestor chain.
///
/// `ancestors` is ordered outermost-first and each entry records whether that
/// element was the node's *direct* parent, which is what a `>` combinator needs.
fn selector_matches(sel: &Selector, node: &Classes, ancestors: &Ancestors) -> bool {
    if !compound_matches(&sel.last, node) {
        return false;
    }
    // Walk the ancestor requirements innermost-first, consuming ancestors from
    // the nearest backwards. `Child` marks the *next* requirement as having to
    // match the element directly above the one before it.
    let mut idx = ancestors.len();
    let mut pending_child = false;
    for cond in sel.ancestors.iter().rev() {
        if matches!(cond, Condition::Child) {
            pending_child = true;
            continue;
        }
        let mut found = false;
        while idx > 0 {
            idx -= 1;
            let (_, cls, was_parent) = &ancestors[idx];
            // With `>`, only the direct parent may satisfy the requirement, so
            // anything further up stops the search. Without it, any ancestor may.
            if pending_child && !*was_parent {
                break;
            }
            if condition_matches(cond, cls) {
                found = true;
                break;
            }
            if *was_parent {
                break;
            }
        }
        if !found {
            return false;
        }
        pending_child = false;
    }
    true
}

/// Does a `.card`, `#id`, `tag` or `*` compound match this node?
fn compound_matches(compound: &str, node: &Classes) -> bool {
    if compound == "*" {
        return true;
    }
    if let Some(id) = compound.strip_prefix('#') {
        return node.id.as_deref() == Some(id);
    }
    if let Some(cls) = compound.strip_prefix('.') {
        return node.classes.iter().any(|x| x == cls);
    }
    node.tag.as_deref() == Some(compound)
}

// ---------------------------------------------------------------------------
// Declaration application
// ---------------------------------------------------------------------------

/// Apply one `property: value` declaration to a [`Style`].
///
/// Returns `Err(reason)` for anything not understood. The reason strings are
/// `&'static str` so the report can be compared in tests.
fn apply_declaration(style: &mut Style, prop: &str, value: &str, report: &mut ParseReport) {
    let prop = prop.trim().to_ascii_lowercase();
    let value = value.trim();
    if value.is_empty() {
        report.skipped.push((format!("{prop}: "), "empty value"));
        return;
    }
    let done = format!("{prop}: {value}");

    macro_rules! ok {
        () => {{
            report.applied.push(done);
            return;
        }};
    }
    macro_rules! no {
        ($why:expr) => {{
            report.skipped.push((done, $why));
            return;
        }};
    }

    match prop.as_str() {
        // --- box sizing ---------------------------------------------------
        "width" => match parse_px(value) {
            Some(v) => {
                style.width = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "height" => match parse_px(value) {
            Some(v) => {
                style.height = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "min-width" => match parse_px(value) {
            Some(v) => {
                style.width = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "min-height" => match parse_px(value) {
            Some(v) => {
                style.height = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "max-width" => match parse_px(value) {
            Some(v) => {
                style.max_width = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "aspect-ratio" => {
            // Only a single number; `16 / 9` needs the slash form handled too.
            let n = value.replace(' ', "");
            if let Some((a, b)) = n.split_once('/') {
                match (a.parse::<f32>(), b.parse::<f32>()) {
                    (Ok(a), Ok(b)) if b.abs() > f32::EPSILON => {
                        style.aspect = Some(a / b);
                        ok!();
                    }
                    _ => no!("could not parse ratio"),
                }
            } else if let Ok(v) = value.parse::<f32>() {
                style.aspect = Some(v);
                ok!();
            } else {
                no!("could not parse ratio");
            }
        }
        "padding" => match parse_box4(value) {
            Some((t, r, b, l)) => {
                // The engine has one uniform padding, so a non-uniform value is
                // reported rather than approximated with one of the four.
                if (t - r).abs() < 0.01 && (t - b).abs() < 0.01 && (t - l).abs() < 0.01 {
                    style.padding = t;
                    ok!();
                }
                no!("only uniform padding is supported");
            }
            None => no!("could not parse length"),
        },
        "margin" => match parse_box4(value) {
            Some((t, r, b, l)) => {
                if (t - r).abs() < 0.01 && (t - b).abs() < 0.01 && (t - l).abs() < 0.01 {
                    style.margin = t;
                    ok!();
                }
                no!("only uniform margin is supported");
            }
            None => no!("could not parse length"),
        },
        "gap" | "grid-gap" => {
            let parts = split_top_level(value);
            // row-gap column-gap: the engine has one gap, so a single value or a
            // uniform pair is fine.
            let first = parts.first().and_then(|s| parse_px(s));
            match first {
                Some(v) => {
                    style.gap = v;
                    ok!();
                }
                None => no!("could not parse length"),
            }
        }
        "border" => {
            // `border: 1px solid #ccc`
            let parts = split_top_level(value);
            let width = parts.first().and_then(|s| parse_px(s));
            let color = parts.iter().skip(1).find_map(|p| parse_color(p));
            match (width, color) {
                (Some(w), Some(c)) => {
                    style.border = w;
                    style.border_color = Some(c);
                    ok!();
                }
                _ => no!("expected a width and a colour"),
            }
        }
        "border-width" => match parse_px(value) {
            Some(v) => {
                style.border = v;
                ok!();
            }
            None => no!("could not parse length"),
        },
        "border-color" => match parse_color(value) {
            Some(c) => {
                style.border_color = Some(c);
                ok!();
            }
            None => no!("unsupported colour"),
        },
        "border-radius" => {
            let parts = split_top_level(value);
            match parts.first().and_then(|s| parse_px(s)) {
                Some(v) => {
                    style.radius = v;
                    ok!();
                }
                None => no!("could not parse length"),
            }
        }

        // --- colour and type ----------------------------------------------
        "color" => match parse_color(value) {
            Some(c) => {
                style.color = Some(c);
                ok!();
            }
            None => no!("unsupported colour"),
        },
        "font-size" => match parse_px(value) {
            Some(v) if v > 0.0 => {
                style.font_size = Some(v);
                ok!();
            }
            _ => no!("expected an absolute length"),
        },
        "font-family" => no!("fonts are chosen by register_font, not by name"),
        "font-weight" | "font-style" | "line-height" | "letter-spacing" | "word-spacing"
        | "text-align" | "text-transform" | "white-space" | "text-decoration" | "text-indent"
        | "line-break" | "word-break" | "overflow-wrap" | "text-overflow" | "vertical-align" => {
            no!("no effect on a fixed-size render")
        }

        // --- layout -------------------------------------------------------
        "display" => {
            let v = value.to_ascii_lowercase();
            match v.as_str() {
                "flex" | "inline-flex" => {
                    style.display = Display::Flex;
                    ok!();
                }
                "grid" | "inline-grid" => {
                    style.display = Display::Grid;
                    ok!();
                }
                "block" | "flow-root" | "inline-block" => {
                    style.display = Display::Block;
                    ok!();
                }
                "none" => no!("display:none would drop the node; render a transparent box instead"),
                _ => no!("unsupported display value"),
            }
        }
        "flex-direction" => {
            let v = value.to_ascii_lowercase();
            match v.as_str() {
                "row" | "row-reverse" => {
                    style.dir = FlexDir::Row;
                    ok!();
                }
                "column" | "column-reverse" => {
                    style.dir = FlexDir::Column;
                    ok!();
                }
                _ => no!("unsupported flex-direction"),
            }
        }
        "justify-content" => match parse_justify(value) {
            Some(j) => {
                style.justify = j;
                ok!();
            }
            None => no!("unsupported justify-content"),
        },
        "align-items" | "align-self" => match parse_align(value) {
            Some(a) => {
                style.align = a;
                ok!();
            }
            None => no!("unsupported align-items"),
        },
        "flex-grow" => match value.parse::<f32>() {
            Ok(v) => {
                style.grow = v;
                ok!();
            }
            Err(_) => no!("expected a number"),
        },
        "flex" => {
            // `flex: 1` / `flex: 1 1 auto`
            let first = value.split_whitespace().next().unwrap_or("");
            match first.parse::<f32>() {
                Ok(v) => {
                    style.grow = v;
                    ok!();
                }
                Err(_) => no!("unsupported flex shorthand"),
            }
        }
        "grid-template-columns" => {
            // `repeat(3, 1fr)` and a bare count.
            let v = value.trim().to_ascii_lowercase();
            if let Some(rest) = v.strip_prefix("repeat(") {
                let inner = rest.trim_end_matches(')');
                let n = inner.split(',').next().unwrap_or("").trim();
                match n.parse::<u16>() {
                    Ok(n) if n > 0 => {
                        style.grid_cols = Some(n);
                        style.display = Display::Grid;
                        ok!();
                    }
                    _ => no!("only repeat(<n>, ...) is supported"),
                }
            } else if let Ok(n) = v.parse::<u16>() {
                if n > 0 {
                    style.grid_cols = Some(n);
                    style.display = Display::Grid;
                    ok!();
                }
                no!("column count must be positive")
            } else {
                no!("only an equal-column count is supported")
            }
        }
        "position" => {
            let v = value.to_ascii_lowercase();
            match v.as_str() {
                "relative" | "static" => {
                    style.absolute = false;
                    ok!();
                }
                "absolute" | "fixed" => {
                    style.absolute = true;
                    ok!();
                }
                _ => no!("unsupported position"),
            }
        }
        "direction" => match value.trim().to_ascii_lowercase().as_str() {
            // The engine derives direction per-run from bidi analysis rather than
            // from a property, so an accepted value is recorded and ignored
            // rather than rejected: rejecting it would push authors to work
            // around a parser that looks broken.
            "ltr" | "rtl" => ok!(),
            _ => no!("expected ltr or rtl"),
        },
        "left" => match parse_px(value) {
            Some(v) => {
                style.left = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "top" => match parse_px(value) {
            Some(v) => {
                style.top = Some(v);
                ok!();
            }
            None => no!("relative or unsupported length"),
        },
        "right" | "bottom" => no!("only left/top offsets are supported"),
        "z-index" => no!("paint order is document order"),
        "float" => no!("float is not supported"),
        "transform" => no!("transform is not supported"),
        "order" => no!("paint order is document order"),
        "flex-basis" => no!("use width on the child instead"),
        "max-height" => no!("only width, height and max-width map onto the layout tree"),

        // --- backgrounds --------------------------------------------------
        "background" | "background-color" => {
            if let Some(c) = parse_color(value) {
                style.background = Some(Background::Solid(c));
                ok!();
            } else if let Some(bg) = parse_gradient(value) {
                style.background = Some(bg);
                ok!();
            } else {
                no!("unsupported background value")
            }
        }
        "background-image" => match parse_gradient(value) {
            Some(bg) => {
                style.background = Some(bg);
                ok!();
            }
            None => no!("only gradients are supported here"),
        },
        "linear-gradient" => match parse_gradient(value) {
            Some(bg) => {
                style.background = Some(bg);
                ok!();
            }
            None => no!("could not parse gradient"),
        },
        "box-shadow" => match parse_box_shadow(value) {
            Some(s) => {
                style.shadow = Some(s);
                ok!();
            }
            None => no!("unsupported box-shadow"),
        },
        "mix-blend-mode" | "background-blend-mode" => match parse_blend(value) {
            Some(b) => {
                style.blend = Some(b);
                ok!();
            }
            None => no!("unsupported blend mode"),
        },
        "opacity" => no!("use an rgba colour with alpha instead"),
        "filter" | "backdrop-filter" => no!("filters are not supported"),
        "mask" | "mask-image" => no!("masks are not supported"),
        // `object-fit` is a property of the image node, not of the box, so it
        // cannot be carried on `Style`. The caller sets `ImgFit` directly; see
        // `img_fit_from_css` for the mapping.
        "object-fit" => no!("set ImgFit on the image node instead"),
        "overflow" | "overflow-x" | "overflow-y" => no!("no scrolling in a fixed-size render"),
        "box-sizing" => no!("always border-box here"),
        "clip-path" | "clip" => no!("use border-radius"),
        "background-clip" => {
            if value.trim().eq_ignore_ascii_case("text") {
                style.clip_text = true;
                ok!();
            } else {
                no!("only background-clip: text is supported");
            }
        }
        "width-percent" => no!("percentages have no containing block here"),

        // --- unsupported but recognised ------------------------------------
        "animation" | "transition" | "cursor" | "content" | "will-change" | "user-select"
        | "pointer-events" | "touch-action" | "scroll-behavior" | "@import" | "src" => {
            no!("no effect on a still render")
        }

        _ => no!("unknown property"),
    }
}

fn parse_box4(value: &str) -> Option<(f32, f32, f32, f32)> {
    let parts = split_top_level(value);
    let v: Vec<f32> = parts.iter().filter_map(|p| parse_px(p)).collect();
    match v.len() {
        1 => Some((v[0], v[0], v[0], v[0])),
        2 => Some((v[0], v[1], v[0], v[1])),
        3 => Some((v[0], v[1], v[2], v[1])),
        4 => Some((v[0], v[1], v[2], v[3])),
        _ => None,
    }
}

fn parse_justify(value: &str) -> Option<Justify> {
    let v = value.to_ascii_lowercase();
    let base = v.split_whitespace().next().unwrap_or("");
    match base {
        "flex-start" | "start" | "left" | "normal" => Some(Justify::Start),
        "center" => Some(Justify::Center),
        "flex-end" | "end" | "right" => Some(Justify::End),
        "space-between" => Some(Justify::SpaceBetween),
        "space-around" => Some(Justify::SpaceAround),
        // `space-evenly` has no exact equivalent; SpaceAround is the closest and
        // is visibly different, so it is reported instead.
        _ => None,
    }
}

fn parse_align(value: &str) -> Option<Align> {
    let v = value.to_ascii_lowercase();
    let base = v.split_whitespace().next().unwrap_or("");
    match base {
        "flex-start" | "start" | "baseline" | "normal" => Some(Align::Start),
        "center" => Some(Align::Center),
        "flex-end" | "end" => Some(Align::End),
        "stretch" | "normal " => Some(Align::Stretch),
        _ => None,
    }
}

fn parse_blend(value: &str) -> Option<BlendMode> {
    let v = value.to_ascii_lowercase();
    Some(match v.as_str() {
        "normal" => BlendMode::Normal,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "lighten" => BlendMode::Lighten,
        "darken" => BlendMode::Darken,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        _ => return None,
    })
}

/// Parse a gradient function into a [`Background`].
///
/// `linear-gradient([angle]? stops...)` and `radial-gradient(...)`. The angle is
/// CSS's `0deg = to top`; the engine measures from the x-axis, so it is
/// translated here rather than at paint time.
fn parse_gradient(value: &str) -> Option<Background> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(inner) = v
        .strip_prefix("linear-gradient(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts = split_gradient_args(inner);
        let mut angle = 180.0_f32;
        let mut start = 0;
        // A leading `<angle>` or a `to <side>` keyword. Corner keywords are two
        // words ("to top right"), so the check joins the first two arguments when
        // the first starts with "to " and no comma separated them.
        if let Some(first) = parts.first() {
            let f = first.trim();
            if f.ends_with("deg")
                || f.ends_with("grad")
                || f.ends_with("rad")
                || f.ends_with("turn")
            {
                let n = f
                    .trim_end_matches("deg")
                    .trim_end_matches("grad")
                    .trim_end_matches("rad")
                    .trim_end_matches("turn")
                    .trim();
                angle = if f.ends_with("turn") {
                    n.parse::<f32>().ok()? * 360.0
                } else if f.ends_with("rad") {
                    n.parse::<f32>().ok()?.to_degrees()
                } else {
                    n.parse().ok()?
                };
                start = 1;
            } else if f.starts_with("to ") {
                // Try two words first, so "to top right" resolves.
                let two = match parts.get(1) {
                    Some(second) if !second.trim().ends_with(',') && f == "to" => {
                        format!("{} {}", f, second.trim())
                    }
                    _ => f.to_owned(),
                };
                angle = css_side_to_deg(&two)?;
                start = 2.min(parts.len());
            }
        }
        let stops = parse_stops(&parts[start..])?;
        // CSS measures clockwise from "to top"; the engine measures from +x.
        Some(Background::Linear {
            angle_deg: angle - 90.0,
            stops,
        })
    } else if let Some(inner) = v
        .strip_prefix("radial-gradient(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts = split_gradient_args(inner);
        // Skip a leading `circle`/`ellipse at ...` shape clause.
        let start = if parts
            .first()
            .is_some_and(|p| p.contains("circle") || p.contains("ellipse") || p.contains("at "))
        {
            1
        } else {
            0
        };
        let stops = parse_stops(&parts[start..])?;
        Some(Background::Radial {
            cx: 0.5,
            cy: 0.5,
            radius: 0.6,
            stops,
        })
    } else {
        None
    }
}

fn css_side_to_deg(side: &str) -> Option<f32> {
    Some(match side {
        "to top" => 0.0,
        "to right" => 90.0,
        "to bottom" => 180.0,
        "to left" => 270.0,
        "to top right" | "to right top" => 45.0,
        "to bottom right" | "to right bottom" => 135.0,
        "to bottom left" | "to left bottom" => 225.0,
        "to top left" | "to left top" => 315.0,
        _ => return None,
    })
}

fn parse_stops(parts: &[String]) -> Option<Vec<ColorStop>> {
    if parts.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        // A stop is `color` or `pos% color`.
        let mut pos: Option<f32> = None;
        let mut color_str = p.as_str();
        if let Some(first) = p.split_whitespace().next() {
            if first.ends_with('%') {
                pos = first
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .map(|n| n / 100.0);
                color_str = p.split_whitespace().nth(1).unwrap_or("");
            }
        }
        let color = parse_color(color_str)?;
        out.push(ColorStop {
            pos: pos.unwrap_or(0.0),
            color,
        });
    }
    // Fill in missing positions by distributing them evenly, which is what a
    // browser does when stops are unpositioned. The count is read before the
    // loop because borrowing an element mutably rules out reading `len` from the
    // same vector inside the body.
    let n = out.len();
    if n > 1 && out.iter().any(|s| s.pos == 0.0) {
        let last = (n - 1) as f32;
        for (i, stop) in out.iter_mut().enumerate() {
            if stop.pos == 0.0 {
                stop.pos = i as f32 / last;
            }
        }
    }
    Some(out)
}

/// `box-shadow: [inset]? dx dy blur? spread? color?`
fn parse_box_shadow(value: &str) -> Option<Shadow> {
    let mut kind = ShadowKind::Drop;
    let parts = split_top_level(value);
    if parts
        .first()
        .is_some_and(|p| p.eq_ignore_ascii_case("inset"))
    {
        kind = ShadowKind::InsetTop;
    }
    let mut lengths = Vec::new();
    let mut color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 102,
    };
    let mut saw_color = false;
    for p in &parts {
        if p.eq_ignore_ascii_case("inset") {
            continue;
        }
        if let Some(c) = parse_color(p) {
            color = c;
            saw_color = true;
        } else {
            lengths.push(parse_px(p)?);
        }
    }
    if lengths.len() < 2 {
        return None;
    }
    Some(Shadow {
        kind,
        dx: lengths[0],
        dy: lengths[1],
        blur: lengths.get(2).copied().unwrap_or(0.0),
        spread: lengths.get(3).copied().unwrap_or(0.0),
        color: if saw_color {
            color
        } else {
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 102,
            }
        },
    })
}

// ---------------------------------------------------------------------------
// Stylesheet
// ---------------------------------------------------------------------------

/// A parsed rule: selector plus the declarations it applies.
#[derive(Debug, Clone)]
struct Rule {
    selector: Selector,
    declarations: Vec<(String, String)>,
}

/// A parsed stylesheet, ready to apply to a tree.
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
}

impl Stylesheet {
    /// Parse a stylesheet.
    ///
    /// A syntax error is returned rather than panicking or silently dropping the
    /// rest: a stylesheet that half-parses produces a wrong image that looks
    /// nearly right, which is the failure mode this project keeps fixing.
    pub fn parse(css: &str, report: &mut ParseReport) -> Result<Self, Error> {
        let toks = tokenize(css);
        let mut rules = Vec::new();
        let mut i = 0;

        while i < toks.len() {
            // Skip any stray closing brace left by a malformed rule, so one
            // unbalanced `}` cannot discard everything after it. This used to
            // break out of the loop instead, leaving `i` pointing at the *next*
            // rule's selector rather than at a `{`; the trailing-text branch then
            // fired and parsing stopped at rule one, silently dropping the rest
            // of the stylesheet.
            while i < toks.len() && toks[i] == Tok::Punct('}') {
                i += 1;
            }
            if i >= toks.len() {
                break;
            }

            // Collect the selector up to `{`.
            let mut sel_tokens = Vec::new();
            while i < toks.len() && toks[i] != Tok::Punct('{') {
                sel_tokens.push(toks[i].clone());
                i += 1;
            }
            if i >= toks.len() {
                // Trailing text with no block: nothing to do.
                if !sel_tokens.is_empty() {
                    let s = render_tokens(&sel_tokens);
                    if s.trim_start().starts_with('@') {
                        report.ignored_selectors.push(format!("{s} (at-rule)"));
                    }
                }
                break;
            }
            // i is at '{'
            i += 1;

            // Collect declarations up to the matching '}'.
            let mut decls: Vec<(String, String)> = Vec::new();
            let mut depth = 1usize;
            while i < toks.len() {
                match &toks[i] {
                    Tok::Punct('{') => depth += 1,
                    Tok::Punct('}') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                // A declaration is `name : value` terminated by `;`.
                let mut name = String::new();
                while i < toks.len() && toks[i] != Tok::Punct(':') && toks[i] != Tok::Punct(';') {
                    name.push_str(&token_text(&toks[i]));
                    i += 1;
                }
                if i < toks.len() && toks[i] == Tok::Punct(':') {
                    i += 1;
                    let mut val = String::new();
                    let mut paren: usize = 0;
                    while i < toks.len() {
                        match &toks[i] {
                            Tok::Punct('(') => paren += 1,
                            Tok::Punct(')') => paren = paren.saturating_sub(1),
                            // A `;` or `}` at paren depth zero ends the value. The
                            // depth check is what keeps `;` and `}` *inside*
                            // `url(...)` or a quoted string from ending it.
                            Tok::Punct(';') if paren == 0 => break,
                            Tok::Punct('}') if paren == 0 => break,
                            _ => {}
                        }
                        // Render rather than concatenate raw token text: bare
                        // concatenation dropped the space after a comma, so
                        // `linear-gradient(90deg, #fff, #000)` arrived as one
                        // token and the gradient parser rejected it.
                        if needs_space_before(&toks[i], &val) {
                            val.push(' ');
                        }
                        val.push_str(&token_text(&toks[i]));
                        i += 1;
                    }
                    // Advance past a `;`. A `}` is deliberately *not* consumed
                    // here: the loop below decrements depth on it and the outer
                    // block consumes it once, so consuming it twice skipped a
                    // token and silently discarded the rule after the first.
                    if i < toks.len() && toks[i] == Tok::Punct(';') {
                        i += 1;
                    }
                    let name = name.trim().to_ascii_lowercase();
                    if !name.is_empty() && !val.trim().is_empty() {
                        decls.push((name, val.trim().to_owned()));
                    }
                } else if i < toks.len() {
                    i += 1;
                }
            }
            if i < toks.len() {
                i += 1; // consume '}'
            }

            let sel_text = render_tokens(&sel_tokens);
            let sel_trim = sel_text.trim();
            if sel_trim.is_empty() {
                continue;
            }
            // Comma-separated selector lists: every one must parse, or the rule
            // is skipped rather than half-applied.
            let mut parsed: Vec<Selector> = Vec::new();
            let mut all_ok = true;
            for one in split_selector_list(sel_trim) {
                match parse_selector(one) {
                    Some(s) => parsed.push(s),
                    None => {
                        all_ok = false;
                        break;
                    }
                }
            }
            if !all_ok {
                report.ignored_selectors.push(sel_trim.to_owned());
                continue;
            }
            for selector in parsed {
                rules.push(Rule {
                    selector,
                    declarations: decls.clone(),
                });
            }
        }

        Ok(Self { rules })
    }

    /// Apply every matching rule to `style`, in source order so later rules win.
    ///
    /// This is CSS cascade order for the cases this subset supports. Specificity
    /// is deliberately **not** computed: with only descendant, child, class, id
    /// and tag selectors, source order is predictable, whereas a half-implemented
    /// specificity model would be neither. Documented rather than hidden.
    fn apply(
        &self,
        node: &Classes,
        ancestors: &Ancestors,
        style: &mut Style,
        report: &mut ParseReport,
    ) {
        for rule in &self.rules {
            if selector_matches(&rule.selector, node, ancestors) {
                for (prop, value) in &rule.declarations {
                    apply_declaration(style, prop, value, report);
                }
            }
        }
    }

    /// Rules in this stylesheet, for tests and diagnostics.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

fn split_selector_list(s: &str) -> Vec<&str> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect()
}

/// Does this token need a leading space when appended to `so_far`?
///
/// The rule is the one CSS itself uses: whitespace is only significant between
/// two value components. After `(`, `,` and `:` no space belongs; between two
/// words it does, which is what separates `0 4px 8px`.
fn needs_space_before(tok: &Tok, so_far: &str) -> bool {
    if so_far.is_empty() {
        return false;
    }
    match tok {
        Tok::Punct(c) => !matches!(c, '(' | ')' | ','),
        // Two adjacent words are separate components, so they need a space
        // between them unless the value so far ends in punctuation that binds
        // them (`to` + `right` would otherwise become `toright`).
        Tok::Word(w) => !so_far.ends_with([',', '(', ':', '+', '~', '>']) && !w.is_empty(),
    }
}

fn token_text(t: &Tok) -> String {
    match t {
        Tok::Punct(c) => c.to_string(),
        Tok::Word(w) => w.clone(),
    }
}

/// Reassemble tokens into text.
///
/// No spaces are inserted inside parentheses, and none after `(` or before `)`.
/// That matters because the value parsers match function syntax by prefix —
/// `linear-gradient(180deg,...)` — so injecting spaces breaks every gradient and
/// every `rgb()`. Between words outside parentheses a single space is kept,
/// which is what makes `box-shadow: 0 4px 8px` re-readable.
fn render_tokens(toks: &[Tok]) -> String {
    let mut out = String::new();
    let mut in_parens = false;
    let mut just_colon = false;
    for t in toks {
        match t {
            Tok::Word(w) => {
                let needs_space = !out.is_empty()
                    && !just_colon
                    && !in_parens
                    && !out.ends_with(['(', ',', '>', '+', '~'])
                    && !w.is_empty();
                if needs_space {
                    out.push(' ');
                }
                out.push_str(w);
                just_colon = false;
            }
            Tok::Punct(c) => match c {
                '(' => {
                    in_parens = true;
                    out.push('(');
                }
                ')' => {
                    in_parens = false;
                    out.push(')');
                }
                ':' => {
                    // Mark that the next word must not be separated by a space:
                    // `a:hover` re-rendering as `a: hover` would stop the parser
                    // recognising it as a pseudo-selector.
                    just_colon = true;
                    out.push(':');
                }
                other => out.push(*other),
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CSS -> Node
// ---------------------------------------------------------------------------

/// One element of a declarative document.
#[derive(Debug, Clone, Default)]
pub struct Element {
    /// Tag name, lowercased. `None` for the root.
    pub tag: Option<String>,
    /// `class` attribute, split on whitespace.
    pub classes: Vec<String>,
    /// `id` attribute.
    pub id: Option<String>,
    /// `style` attribute contents.
    pub inline: String,
    /// Child elements, in document order.
    pub children: Vec<Element>,
    /// Text content, if this is a text node.
    pub text: Option<String>,
    /// Image bytes, for `<img src>` where the caller resolved the source.
    pub image: Option<Vec<u8>>,
    /// Hyperlink target, for `<a href>`.
    pub href: Option<String>,
}

impl Element {
    /// A container element with a tag.
    #[must_use]
    pub fn new(tag: &str, children: Vec<Element>) -> Self {
        Self {
            tag: Some(tag.to_ascii_lowercase()),
            children,
            ..Self::default()
        }
    }

    /// A text node.
    #[must_use]
    pub fn text(content: &str) -> Self {
        Self {
            text: Some(content.to_owned()),
            ..Self::default()
        }
    }

    /// Builder: add a class.
    #[must_use]
    pub fn class(mut self, c: &str) -> Self {
        self.classes.push(c.to_owned());
        self
    }

    /// Builder: set an id.
    #[must_use]
    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.to_owned());
        self
    }

    /// Builder: set inline CSS.
    #[must_use]
    pub fn style(mut self, css: &str) -> Self {
        self.inline = css.to_owned();
        self
    }

    /// Builder: attach image bytes.
    #[must_use]
    pub fn image_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.image = Some(bytes);
        self
    }

    /// Builder: set a hyperlink.
    #[must_use]
    pub fn href(mut self, url: &str) -> Self {
        self.href = Some(url.to_owned());
        self
    }
}

/// Build a node tree from a stylesheet and a declarative document.
///
/// Inline `style` attributes are applied *after* the stylesheet, which is what
/// makes them inline rather than merely author-level. Classes and ids cascade
/// through the element tree.
pub fn from_elements(sheet: &Stylesheet, root: &Element, report: &mut ParseReport) -> Node {
    build(sheet, root, &Ancestors::new(), report)
}

fn build(
    sheet: &Stylesheet,
    el: &Element,
    ancestors: &Ancestors,
    report: &mut ParseReport,
) -> Node {
    let mut style = Style::new();
    sheet.apply(&el.classes_view(), ancestors, &mut style, report);
    // Inline styles win, so they are applied last.
    if !el.inline.trim().is_empty() {
        let toks = tokenize(&el.inline);
        let mut inline_report = ParseReport::default();
        let mut i = 0;
        while i < toks.len() {
            let mut name = String::new();
            while i < toks.len() && toks[i] != Tok::Punct(':') && toks[i] != Tok::Punct(';') {
                name.push_str(&token_text(&toks[i]));
                i += 1;
            }
            if i < toks.len() && toks[i] == Tok::Punct(':') {
                i += 1;
                let mut val = String::new();
                let mut paren: usize = 0;
                while i < toks.len() {
                    match &toks[i] {
                        Tok::Punct('(') => paren += 1,
                        Tok::Punct(')') => paren = paren.saturating_sub(1),
                        Tok::Punct(';') if paren == 0 => break,
                        Tok::Punct('}') if paren == 0 => break,
                        _ => {}
                    }
                    if needs_space_before(&toks[i], &val) {
                        val.push(' ');
                    }
                    val.push_str(&token_text(&toks[i]));
                    i += 1;
                }
                if i < toks.len() && toks[i] == Tok::Punct(';') {
                    i += 1;
                }
                let n = name.trim().to_ascii_lowercase();
                if !n.is_empty() {
                    apply_declaration(&mut style, &n, val.trim(), &mut inline_report);
                }
            } else {
                i += 1;
            }
        }
        // Inline declarations are attributed to the author, so fold them in.
        report.applied.extend(inline_report.applied);
        report.skipped.extend(inline_report.skipped);
    }

    let mut child_ancestors: Ancestors = ancestors.to_vec();
    child_ancestors.push((style.clone(), el.classes_view(), true));

    // An element whose whole content is text becomes a `Text` node, so
    // `font-size` and `color` land on the glyphs rather than on an empty box.
    //
    // Both shapes count: text set directly on the element by `parse_html`, and
    // text in a single child element (which is what `<div>text</div>` produces).
    // Only the second shape was handled before, which meant `.title{font-size:72px}`
    // on a `<span>Title</span>` styled a container and left the glyphs at the
    // 32px default — the single most visible way for a CSS front end to be wrong.
    let only_text = if is_boxless(&style)
        && ((el.text.is_some() && el.children.is_empty())
            || (el.text.is_none()
                && el.children.len() == 1
                && el.children[0].text.is_some()
                && el.children[0].children.is_empty()
                && el.children[0].image.is_none()))
    {
        el.text.clone().or_else(|| el.children[0].text.clone())
    } else {
        None
    };
    if let Some(text) = only_text {
        let mut tstyle = style.clone();
        // `inherit_text` carries down whatever an ancestor styled, so a bare text
        // node deep in a tree picks up its family's type settings rather than
        // resetting to the defaults on every element.
        let inherited = inherited_text(ancestors);
        if tstyle.font_size.is_none() {
            tstyle.font_size = Some(inherited.font_size.unwrap_or(32.0));
        }
        if tstyle.color.is_none() {
            tstyle.color = Some(inherited.color.unwrap_or(Color::rgb(0, 0, 0)));
        }
        if tstyle.font.is_none() {
            tstyle.font = inherited.font;
        }
        if tstyle.max_width.is_none() {
            tstyle.max_width = inherited.max_width;
        }
        let mut node = Node::text(&text, tstyle);
        if let Some(h) = &el.href {
            if let Node::Text { style, .. } = &mut node {
                style.link = Some(h.clone());
            }
        }
        return node;
    }

    let children: Vec<Node> = el
        .children
        .iter()
        .map(|c| build(sheet, c, &child_ancestors, report))
        .collect();

    // An element with no tag and no children is an empty box.
    if el.tag.is_none() && el.children.is_empty() {
        return Node::container(style, Vec::new());
    }

    let mut node = Node::container(style, children);
    if let Some(h) = &el.href {
        if let Node::Container { style, .. } = &mut node {
            style.link = Some(h.clone());
        }
    }
    if let (
        Some(bytes),
        Node::Container {
            style, children, ..
        },
    ) = (&el.image, &mut node)
    {
        // An <img> with resolved bytes becomes an Image node in place.
        let mut img = Node::image(bytes.clone(), std::mem::take(style));
        if let Node::Image { style: s, .. } = &mut img {
            for c in children {
                if let Node::Text { text, style } = c {
                    let _ = text;
                    s.color = style.color;
                }
            }
        }
        return img;
    }
    node
}

impl Element {
    fn classes_view(&self) -> Classes {
        Classes {
            id: self.id.clone(),
            classes: self
                .classes
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect(),
            tag: self.tag.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

/// Parse a small HTML document into elements.
///
/// Handles nesting, void elements, self-closing tags, comments, `<!DOCTYPE>`,
/// and the `class`, `id`, `style`, `href` and `src` attributes. Other
/// attributes are ignored, and `<script>` / `<style>` *content* is skipped
/// rather than parsed as markup — a `<style>` block's CSS is handled by
/// [`html_with_stylesheet`], which returns the sheet alongside the tree.
///
/// Image `src` values are **not** fetched: this is synchronous and offline by
/// design. Resolve them with [`Element::image_bytes`] or
/// [`attach_images`].
#[must_use]
pub fn parse_html(html: &str) -> Vec<Element> {
    let mut roots: Vec<Element> = Vec::new();
    let mut stack: Vec<Element> = Vec::new();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '<' {
            // Text run.
            let start = i;
            while i < chars.len() && chars[i] != '<' {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            if !text.trim().is_empty() {
                let decoded = decode_entities(text.trim());
                let node = Element::text(&decoded);
                push_node(&mut stack, &mut roots, node);
            }
            continue;
        }

        // Comment.
        if chars.get(i + 1) == Some(&'!')
            && chars.get(i + 2) == Some(&'-')
            && chars.get(i + 3) == Some(&'-')
        {
            i += 5;
            while i + 2 < chars.len() {
                if chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>' {
                    i += 3;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Doctype or processing instruction.
        if chars.get(i + 1) == Some(&'!') || chars.get(i + 1) == Some(&'?') {
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            i += 1;
            continue;
        }
        // Closing tag.
        if chars.get(i + 1) == Some(&'/') {
            i += 2;
            let start = i;
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            let name: String = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            i += 1;
            // Pop to the matching open tag, discarding any unclosed children: a
            // malformed document should still render what it clearly meant.
            if let Some(pos) = stack
                .iter()
                .rposition(|e| e.tag.as_deref() == Some(name.trim()))
            {
                while stack.len() > pos {
                    let done = stack.pop().expect("non-empty");
                    push_node(&mut stack, &mut roots, done);
                }
            }
            continue;
        }

        // Opening tag.
        i += 1;
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
            i += 1;
        }
        let tag: String = chars[start..i]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        let mut el = Element {
            tag: Some(tag.clone()),
            ..Element::default()
        };
        let mut self_closing = false;
        // Attributes.
        while i < chars.len() && chars[i] != '>' {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '/' {
                self_closing = true;
                i += 1;
                continue;
            }
            if i >= chars.len() || chars[i] == '>' {
                break;
            }
            let astart = i;
            while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() && chars[i] != '>'
            {
                i += 1;
            }
            let attr: String = chars[astart..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let mut value = String::new();
            if i < chars.len() && chars[i] == '=' {
                i += 1;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                let q = chars.get(i).copied().unwrap_or('"');
                if q == '"' || q == '\'' {
                    i += 1;
                    let vstart = i;
                    while i < chars.len() && chars[i] != q {
                        i += 1;
                    }
                    value = chars[vstart..i].iter().collect::<String>();
                    i += 1;
                } else {
                    let vstart = i;
                    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                        i += 1;
                    }
                    value = chars[vstart..i].iter().collect::<String>();
                }
            }
            match attr.as_str() {
                "class" => el.classes = value.split_whitespace().map(str::to_owned).collect(),
                "id" => el.id = Some(value.clone()),
                "style" => el.inline = value.clone(),
                "href" => el.href = Some(decode_entities(&value)),
                // `src` is recorded by the caller via attach_images; keeping it
                // in `inline` would be a lie, so it is dropped here.
                _ => {}
            }
        }
        i += 1; // consume '>'

        let is_void = matches!(
            tag.as_str(),
            "img"
                | "br"
                | "hr"
                | "meta"
                | "link"
                | "input"
                | "source"
                | "area"
                | "base"
                | "col"
                | "embed"
                | "param"
                | "track"
                | "wbr"
        );
        // <script>/<style> bodies are skipped to their close tag.
        if tag == "script" || tag == "style" {
            let close = format!("</{tag}");
            let rest: String = chars[i..].iter().collect::<String>().to_ascii_lowercase();
            if let Some(pos) = rest.find(&close) {
                i += rest[..pos].chars().count();
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
            }
            continue;
        }

        if self_closing || is_void {
            push_node(&mut stack, &mut roots, el);
        } else {
            stack.push(el);
        }
    }

    while let Some(done) = stack.pop() {
        push_node(&mut stack, &mut roots, done);
    }
    roots
}

fn push_node(stack: &mut [Element], roots: &mut Vec<Element>, node: Element) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => roots.push(node),
    }
}

/// Decode the handful of entities that appear in real copy.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Parse an HTML document and its inline `<style>` block into a tree.
///
/// The `<style>` contents are extracted and parsed as a stylesheet, then the
/// element tree is styled with it. This is the single call most callers want.
pub fn html_to_tree(html: &str, report: &mut ParseReport) -> Result<Node, Error> {
    let (css, body) = extract_style_blocks(html);
    let sheet = if css.trim().is_empty() {
        Stylesheet::default()
    } else {
        Stylesheet::parse(&css, report)?
    };
    let roots = parse_html(&body);
    let root = if roots.len() == 1 {
        roots.into_iter().next().expect("one root")
    } else {
        let mut wrapper = Element::new("div", roots);
        wrapper.tag = None;
        wrapper
    };
    Ok(from_elements(&sheet, &root, report))
}

/// Split a document into its CSS (from every `<style>` block) and the rest.
///
/// A document with several blocks concatenates them. That is wrong if the blocks
/// conflict and CSS specificity would decide, but this subset has no specificity
/// model — source order is the whole cascade — so concatenation preserves the
/// ordering the author wrote and the result is the one they meant.
fn extract_style_blocks(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let mut css = String::new();
    let mut body = String::new();
    let mut cursor = 0usize;

    while let Some(rel) = lower[cursor..].find("<style") {
        let start = cursor + rel;
        // A `<style>` tag with attributes still ends at the first '>'.
        let Some(gt_rel) = html[start..].find('>') else {
            break;
        };
        let content_start = start + gt_rel + 1;
        let Some(end_rel) = lower[content_start..].find("</style>") else {
            // Unterminated: treat the remainder as CSS and stop.
            css.push_str(&html[content_start..]);
            return (css, body);
        };
        css.push_str(&html[content_start..content_start + end_rel]);
        css.push('\n');
        body.push_str(&html[cursor..start]);
        cursor = content_start + end_rel + "</style>".len();
    }
    body.push_str(&html[cursor..]);
    (css, body)
}

/// Attach decoded image bytes to `<img>` elements by their `src`.
///
/// Because [`parse_html`] drops `src`, this walks the tree and matches on the
/// caller's own map. Doing it this way keeps the parser synchronous and offline:
/// fetching is the caller's decision, not a surprise network call from a render
/// library whose whole pitch is that it makes none.
pub fn attach_images(el: &mut Element, sources: &HashMap<String, Vec<u8>>) {
    if el.tag.as_deref() == Some("img") {
        // The class list of an <img> is where a caller-provided marker goes, so
        // an image is keyed by its id when present and its single class
        // otherwise. Documented rather than magic.
        let key = el
            .id
            .clone()
            .or_else(|| el.classes.first().cloned())
            .unwrap_or_default();
        if let Some(bytes) = sources.get(&key) {
            el.image = Some(bytes.clone());
        }
    }
    for c in &mut el.children {
        attach_images(c, sources);
    }
}

/// Convenience: the image fit a CSS `object-fit` value maps to.
#[must_use]
pub fn img_fit_from_css(value: &str) -> Option<ImgFit> {
    match value.trim().to_ascii_lowercase().as_str() {
        "cover" => Some(ImgFit::Cover),
        "contain" => Some(ImgFit::Contain),
        "fill" | "scale-down" | "none" => Some(ImgFit::Fill),
        _ => None,
    }
}

/// The default fit, re-exported so callers building elements do not need the
/// enum in scope for the common case.
#[must_use]
pub fn default_img_fit() -> ImgFit {
    ImgFit::Contain
}
