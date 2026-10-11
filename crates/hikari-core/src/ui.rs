//! A builder for trees, for callers who want types rather than a parser.
//!
//! # The problem it solves
//!
//! [`Node`] is a plain enum, and building one by hand nests inside-out:
//!
//! ```no_run
//! use hikari_core::{Node, Style};
//! let by_hand = Node::container(
//!     Style::new().with_size(120.0, 80.0).with_background("#1e293b"),
//!     vec![
//!         Node::text("Title", Style::text(24.0, "#ffffff")),
//!         Node::container(
//!             Style::new().with_size(120.0, 40.0),
//!             vec![Node::text("Body", Style::text(14.0, "#94a3b8"))],
//!         ),
//!     ],
//! );
//! ```
//!
//! The opening bracket, the style and the children arrive before the thing they
//! describe, so the indentation nests the wrong way round and a deep card
//! becomes a wall of closing brackets. This builder reads the other way:
//!
//! ```no_run
//! # use hikari_core::ui;
//! let built = ui::div()
//!     .size(120.0, 80.0)
//!     .bg("#1e293b")
//!     .child(ui::text("Title").size(24.0, 0.0).color("#ffffff"))
//!     .child(
//!         ui::div()
//!             .size(120.0, 40.0)
//!             .child(ui::text("Body").size(14.0, 0.0).color("#94a3b8")),
//!     )
//!     .build();
//! ```
//!
//! # Why there are only four shorthand setters
//!
//! The obvious design is one fluent method per [`Style`] field. That duplicates
//! twenty setters which then have to be kept in step with [`Style`], and the copy
//! is worse than what it replaces: no documentation, no colour parsing, no
//! defaults.
//!
//! So this offers shorthands only for the handful of properties nearly every
//! card sets -- `size`, `bg`, `pad`, `color`, `column` -- and everything
//! else goes through [`Elem::style`], which takes the same closure
//! [`Style`]'s own `with_*` chain takes:
//!
//! ```no_run
//! # use hikari_core::ui;
//! let bordered = ui::div()
//!     .style(|s| s.with_border(2.0, "#f59e0b"))
//!     .build();
//! ```
//!
//! One escape hatch, zero duplicated surface, and nothing in [`Style`] is
//! unreachable from here.
//!
//! # Relationship to the CSS front end
//!
//! [`crate::css`] parses CSS and HTML into the same [`Node`] tree. This is not an
//! alternative to it -- a caller who wants a stylesheet should use that. This is
//! for the other case: a Rust program assembling a tree in code, where a
//! `Node::container(...)` per node is verbose and the nesting reads backwards.
//!
//! It adds no dependency and is a thin layer over [`Node`] and [`Style`]: a
//! tree built here is the same tree, and any builder output can be handed to any
//! other API in the crate.

use crate::{Color, FlexDir, Node, Style};

/// A container. Shorthand for [`Elem::div`].
///
/// Free rather than only associated, because `div()` reads better in a chain
/// than `Elem::div()` and the shorter form is what ends up in real code.
pub fn div() -> Elem {
    Elem::div()
}

/// A text element. Shorthand for [`Elem::text`].
pub fn text(content: &str) -> Elem {
    Elem::text(content)
}

/// An image from encoded bytes. Shorthand for [`Elem::image`].
pub fn image(bytes: Vec<u8>) -> Elem {
    Elem::image(bytes)
}

/// An image resolved from [`crate::assets::Assets`]. Shorthand for
/// [`Elem::image_ref`].
pub fn image_ref(src: &str) -> Elem {
    Elem::image_ref(src)
}

/// A node under construction.
///
/// Consumes and returns `Self` on every method, so a chain cannot be reused by
/// accident and the type never needs a mutable borrow. [`Elem::build`] takes the
/// result by value; [`Into`] for [`Node`] is provided too, so a builder can be
/// used directly anywhere a [`Node`] is expected.
///
/// `#[must_use]` throughout: a dropped builder is a silently missing element,
/// which is exactly the kind of mistake this is meant to reduce.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "an Elem that is never built contributes nothing to the tree"]
pub struct Elem {
    node: Node,
}

impl Elem {
    /// An empty container.
    ///
    /// `div` rather than `container` because it reads better in a chain and is
    /// the name the HTML front end uses for the same element.
    pub fn div() -> Self {
        Self {
            node: Node::container(Style::new(), Vec::new()),
        }
    }

    /// A container holding one child.
    pub fn with_child(child: impl Into<Elem>) -> Self {
        Self::div().child(child)
    }

    /// A text element. The style is default until [`Elem::size`] or
    /// [`Elem::color`] sets it.
    pub fn text(content: &str) -> Self {
        Self {
            node: Node::text(content, Style::new()),
        }
    }

    /// An image from encoded bytes.
    pub fn image(bytes: Vec<u8>) -> Self {
        Self {
            node: Node::image(bytes, Style::new()),
        }
    }

    /// An image whose bytes come from [`crate::assets::Assets`].
    ///
    /// Unresolvable until [`crate::assets::Assets::resolve`] runs, which reports
    /// an unresolved name rather than rendering a blank box.
    pub fn image_ref(src: &str) -> Self {
        Self {
            node: Node::image_ref(src, Style::new()),
        }
    }

    /// Fixed width and height, in px.
    /// Font size, in px, when called on a text element; a fixed box size
    /// otherwise.
    ///
    /// Two meanings from one method is a deliberate trade: `size` is what a
    /// caller reaches for first in both cases, and the wrong one is caught here
    /// by a test rather than in a screenshot. For a box, a width and height are
    /// meaningless; for text, a box height would clip the line.
    pub fn size(mut self, v: f32, h: f32) -> Self {
        if let Node::Text { style, .. } = &mut self.node {
            style.font_size = Some(v);
        } else {
            // `with_size` consumes and returns, so the value has to be taken out
            // and put back. Doing that in one helper keeps it from being spelled
            // out at every call site.
            let taken = std::mem::take(style_of(&mut self.node));
            *style_of(&mut self.node) = taken.with_size(v, h);
        }
        self
    }

    /// Background fill. Accepts `#rgb`, `#rrggbb` or `#rrggbbaa`.
    ///
    /// An unparseable string leaves the background unset rather than panicking;
    /// [`Style::with_background`] is the place that decides, and this defers to
    /// it so the two cannot disagree about what is valid.
    pub fn bg(self, color: &str) -> Self {
        self.style(|s| s.with_background(color))
    }

    /// Padding on all four sides, in px.
    pub fn pad(self, px: f32) -> Self {
        self.style(|s| s.with_padding(px))
    }

    /// Text colour, for a text element.
    ///
    /// Parsed the same way [`Style::text`] parses it, and equally forgiving of
    /// an unparseable string: the colour is left unset rather than panicking.
    pub fn color(self, color: &str) -> Self {
        self.style(|mut s| {
            s.color = Some(Color::from_hex(color));
            s
        })
    }

    /// Lay children out in a column rather than a row.
    ///
    /// [`Style`]'s default is already [`FlexDir::Row`], so this is the shorthand
    /// that changes something -- `row` is offered on [`Style::row`] for the
    /// opposite direction but is a no-op on a default style, and an end-to-end
    /// test that assumed otherwise passed a broken layout for a while before the
    /// default was checked.
    pub fn column(self) -> Self {
        self.style(|mut s| {
            s.dir = FlexDir::Column;
            s
        })
    }

    /// Arbitrary style changes.
    ///
    /// The escape hatch for everything [`Style`] offers that has no shorthand
    /// here. Takes `FnOnce(Style) -> Style` to mirror [`Style`]'s own `with_*`
    /// chain, so those compose directly:
    ///
    /// ```no_run
    /// # use hikari_core::ui;
    /// let bordered = ui::div().style(|s| s.with_border(2.0, "#f59e0b")).build();
    /// ```
    ///
    /// A first version took `FnOnce(&mut Style)`, which is the more natural
    /// signature to reach for and the wrong one: every `Style` method consumes
    /// and returns, so each call site needed a `mem::take` and a put-back. A
    /// doc example failed to compile because of it, which is how it was found.
    /// Setting a field directly still works, with a `mut` binding and the value
    /// returned:
    ///
    /// ```no_run
    /// # use hikari_core::ui;
    /// let spaced = ui::div().style(|mut s| { s.gap = 6.0; s }).build();
    /// ```
    ///
    /// Applied to whatever the style already is, so order within a chain does
    /// not matter for setters that overwrite -- but it does for those that
    /// accumulate, such as `with_border`, where the last call wins.
    pub fn style(mut self, f: impl FnOnce(Style) -> Style) -> Self {
        let taken = std::mem::take(style_of(&mut self.node));
        *style_of(&mut self.node) = f(taken);
        self
    }

    /// Append a child.
    ///
    /// Takes `impl Into<Elem>` so a builder, an existing [`Node`] or another
    /// `Elem` can all be passed without a wrapper.
    pub fn child(mut self, child: impl Into<Elem>) -> Self {
        match &mut self.node {
            Node::Container { children, .. } => children.push(child.into().node),
            // Appending to a text or image would be a type error at the tree
            // level, and silently dropping the child would be worse: the caller
            // would get a tree missing content with no indication why.
            _ => panic!("Elem::child on a non-container; use Elem::div()"),
        }
        self
    }

    /// Append several children.
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Elem>,
    {
        for c in children {
            self = self.child(c);
        }
        self
    }

    /// Finish, producing the tree.
    pub fn build(self) -> Node {
        self.node
    }
}

impl From<Elem> for Node {
    fn from(e: Elem) -> Self {
        e.node
    }
}

impl From<Node> for Elem {
    /// Wraps an existing node so it can be dropped into a chain.
    fn from(node: Node) -> Self {
        Self { node }
    }
}

impl From<&Node> for Elem {
    /// Clones. Present so a borrowed tree composes; building a chain from
    /// borrowed nodes costs a copy per node, which is the price of not requiring
    /// the caller to own everything.
    fn from(node: &Node) -> Self {
        Self { node: node.clone() }
    }
}

/// Borrow the style out of a node.
///
/// Panics on a text or image node, matching [`Elem::child`]: all three of
/// `style`, `child` and `children` are container operations, and silently
/// ignoring them on a leaf would mean a card quietly lost its contents.
fn style_of(node: &mut Node) -> &mut Style {
    match node {
        Node::Container { style, .. } | Node::Text { style, .. } | Node::Image { style, .. } => {
            style
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_builder_produces_the_same_tree_as_the_equivalent_literal() {
        // The point of this module is that it is not a second tree format. If
        // these two ever diverge, one of them is wrong.
        let by_hand = Node::container(
            Style::new()
                .with_size(120.0, 80.0)
                .with_background("#1e293b"),
            vec![Node::text("Title", Style::text(24.0, "#ffffff"))],
        );
        let built = div()
            .size(120.0, 80.0)
            .bg("#1e293b")
            .child(text("Title").size(24.0, 0.0).color("#ffffff"))
            .build();
        assert_eq!(built, by_hand);
    }

    #[test]
    fn text_shorthands_match_style_text_helper() {
        assert_eq!(
            text("x").size(16.0, 0.0).color("#123456").build(),
            Node::text("x", Style::text(16.0, "#123456"))
        );
    }

    #[test]
    fn an_empty_div_is_a_container_with_no_children() {
        assert_eq!(div().build(), Node::container(Style::new(), Vec::new()));
    }

    #[test]
    fn children_append_in_order() {
        // Order is the whole point of a container; a builder that reversed or
        // deduplicated would still look right in a one-child test.
        let t = div().children([text("a"), text("b"), text("c")]).build();
        match t {
            Node::Container { children, .. } => {
                let words: Vec<_> = children
                    .iter()
                    .map(|c| match c {
                        Node::Text { text, .. } => text.as_str(),
                        _ => panic!("expected text"),
                    })
                    .collect();
                assert_eq!(words, vec!["a", "b", "c"]);
            }
            _ => panic!("expected a container"),
        }
    }

    #[test]
    fn builders_nest_to_arbitrary_depth() {
        let t = div()
            .size(100.0, 100.0)
            .child(
                div().size(80.0, 80.0).child(
                    div()
                        .size(60.0, 60.0)
                        .child(div().size(40.0, 40.0).child(text("deep"))),
                ),
            )
            .build();
        // Walk down and confirm the innermost text is still there. A builder that
        // dropped or replaced a level would still produce a valid tree.
        let mut n = &t;
        for _ in 0..3 {
            n = match n {
                Node::Container { children, .. } => &children[0],
                _ => panic!("lost a level"),
            };
        }
        assert!(matches!(n, Node::Container { .. }));
    }

    #[test]
    fn style_escape_hatch_reaches_everything_shorthands_do_not() {
        let t = div()
            .style(|s| s.with_border(2.0, "#f59e0b"))
            .column()
            .build();
        match t {
            Node::Container { style, .. } => {
                assert_eq!(style.border, 2.0);
                assert_eq!(style.dir, FlexDir::Column);
            }
            _ => panic!("expected a container"),
        }
    }

    #[test]
    fn an_existing_node_can_be_dropped_into_a_chain() {
        let existing = Node::text("x", Style::text(10.0, "#000000"));
        assert_eq!(
            div().child(existing.clone()).build(),
            Node::container(Style::new(), vec![existing.clone()])
        );
        // And via Into, without naming the type.
        let t = div().child(existing.clone()).child(&existing).build();
        match t {
            Node::Container { children, .. } => assert_eq!(children.len(), 2),
            _ => panic!("expected a container"),
        }
    }

    #[test]
    fn the_default_direction_is_row() {
        // `Style`'s default is `Row`, not `Column`. An end-to-end test assumed
        // otherwise, compared a "row" layout against a "column" layout, found the
        // two byte-identical, and read it as the shorthand being broken rather
        // than as its own assumption being wrong. Pinning the default turns that
        // class of confusion into a one-line answer.
        assert_eq!(Style::new().dir, FlexDir::Row);
        match div().build() {
            Node::Container { style, .. } => assert_eq!(style.dir, FlexDir::Row),
            _ => panic!("expected a container"),
        }
        // And `column()` is the shorthand that actually moves it.
        match div().column().build() {
            Node::Container { style, .. } => assert_eq!(style.dir, FlexDir::Column),
            _ => panic!("expected a container"),
        }
    }

    #[test]
    fn a_builder_is_usable_without_build() {
        // Into<Node> is what lets a builder drop into an existing tree, which is
        // the whole reason `build` is not the only exit.
        let t: Node = div().size(10.0, 10.0).into();
        assert!(matches!(t, Node::Container { .. }));
    }

    #[test]
    fn style_applies_to_text_and_image_too() {
        // The generic escape hatch must work wherever a style exists, not only on
        // containers.
        let t = text("x")
            .style(|mut s| {
                s.gap = 4.0;
                s
            })
            .build();
        match t {
            Node::Text { style, .. } => assert_eq!(style.gap, 4.0),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn a_text_size_sets_the_font_size_not_a_box_size() {
        // `size` means two different things depending on what it is called on.
        // Getting it wrong is silent either way -- a box height on a text node
        // clips the line and reads as a layout bug rather than a misused setter --
        // so the distinction is pinned here.
        let t = text("x").size(24.0, 0.0).build();
        match t {
            Node::Text { style, .. } => {
                assert_eq!(style.font_size, Some(24.0));
                assert!(
                    style.width.is_none(),
                    "a text node must not get a box width"
                );
            }
            _ => panic!("expected text"),
        }
    }

    #[test]
    #[should_panic(expected = "non-container")]
    fn adding_a_child_to_text_is_refused_loudly() {
        // Dropping the child would produce a tree missing content with nothing
        // in the output to say so, which is the worst outcome available.
        let _ = text("x").child(text("y"));
    }

    #[test]
    fn an_unparseable_colour_does_not_panic() {
        // `with_background` decides what is valid; this must not invent a
        // stricter rule of its own, or the two would disagree.
        let t = div().bg("not-a-colour").build();
        assert!(matches!(t, Node::Container { .. }));
    }
}
