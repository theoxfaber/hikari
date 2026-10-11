//! Resolve image references into bytes, before a render.
//!
//! # Why this exists and why it is not built in
//!
//! A render needs bytes; a document names URLs. Something has to bridge the two,
//! and the obvious thing to build is a fetch inside the renderer. That is
//! precisely the wrong shape for this library, whose whole pitch is that a render
//! makes no network calls:
//!
//! * A render becomes non-deterministic the moment it can fail because a server
//!   is down. The determinism guarantee here is byte-identical output from the
//!   same input, and that cannot survive an implicit fetch.
//! * A renderer that blocks on I/O has no useful backpressure, and a caller
//!   rendering 200 cards would issue 200 independent, uncoordinated fetches.
//! * Errors from a fetch have nowhere to go mid-paint, so they would have to
//!   become a blank box -- the hardest kind of failure to diagnose.
//!
//! So fetching stays the caller's decision, and this is the shape that decision
//! takes: build a map, hand it over, render as before.
//!
//! ```no_run
//! use hikari_core::assets::Assets;
//! use hikari_core::{Node, Style};
//!
//! # fn fetch(_url: &str) -> Vec<u8> { Vec::new() }
//! # fn main() -> Result<(), hikari_core::Error> {
//! // Declared before anything is fetched.
//! let tree = Node::container(
//!     Style::new().with_size(120.0, 60.0),
//!     vec![Node::image_ref("logo", Style::new())],
//! );
//!
//! // The caller fetches; the library is handed the bytes.
//! let mut assets = Assets::new();
//! assets.insert("logo", "https://example.com/logo.png", fetch("https://example.com/logo.png"));
//!
//! // Resolve once, then render as usual -- the renderer takes no part.
//! let resolved = assets.resolve(&tree)?;
//! # let _ = resolved;
//! # Ok(())
//! # }
//! ```
//!
//! Nothing here is required. A tree with no assets renders exactly as before.

use std::collections::BTreeMap;

/// Resolved asset bytes, keyed by name.
///
/// A `BTreeMap` rather than a `HashMap` so iteration order is deterministic:
/// rendering the same tree with the same assets visits images in the same order
/// on every run, which a hash map would not guarantee.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assets {
    by_name: BTreeMap<String, Vec<u8>>,
}

impl Assets {
    /// An empty set. Rendering with it is identical to not using assets at all.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve `url` and store the bytes under `name`.
    ///
    /// Takes the fetch result rather than performing one. That is the whole
    /// design: the caller owns the HTTP client, the timeout, the retry policy and
    /// the cache, because a library choosing any of those would be choosing them
    /// for every caller.
    ///
    /// Returns the byte length, which is usually what a caller logging its own
    /// fetches wants.
    pub fn insert<N: Into<String>, B: Into<Vec<u8>>>(
        &mut self,
        name: N,
        _url: &str,
        bytes: B,
    ) -> usize {
        let bytes = bytes.into();
        let len = bytes.len();
        self.by_name.insert(name.into(), bytes);
        len
    }

    /// Store bytes for a name that was fetched elsewhere.
    #[must_use]
    pub fn with<N: Into<String>, B: Into<Vec<u8>>>(mut self, name: N, bytes: B) -> Self {
        self.insert(name, "", bytes);
        self
    }

    /// Bytes for `name`, if present.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.by_name.get(name).map(Vec::as_slice)
    }

    /// Whether `name` is present.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Names in sorted order, so a caller can report what it resolved.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.by_name.keys().map(String::as_str).collect()
    }

    /// Number of resolved assets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether anything has been resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Total bytes across every asset.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.by_name.values().map(Vec::len).sum()
    }

    /// Assets referenced by `tree` that are not present here.
    ///
    /// The useful half of a preflight check: it answers "what am I about to
    /// render without" before the render, rather than discovering it as a blank
    /// box afterwards.
    #[must_use]
    pub fn missing(&self, tree: &crate::Node) -> Vec<String> {
        let mut out = Vec::new();
        collect_refs(tree, &mut |name| {
            if !self.contains(name) && !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
            }
        });
        out
    }

    /// Fill every `src` reference in `tree` from this set, returning a new tree.
    ///
    /// The renderer is deliberately not involved: it never learns that assets
    /// exist, so it cannot grow a fetch, a cache or a timeout by accident. A
    /// caller resolves once and then renders as usual.
    ///
    /// An unresolved reference is an error rather than an empty image, because a
    /// missing picture that renders as a blank box is indistinguishable from a
    /// picture that is genuinely transparent -- and finding that out by looking
    /// at the output is the worst way to find out.
    pub fn resolve(&self, tree: &crate::Node) -> Result<crate::Node, crate::Error> {
        resolve_node(tree, self, true)
            .map_err(|missing| crate::Error::Asset(format!("no asset named {missing:?}")))
    }

    /// Resolve, but report unresolved references as a list rather than failing
    /// on the first.
    ///
    /// Useful in a preflight: one pass tells a caller everything it still needs
    /// to fetch, instead of one fetch per render attempt.
    #[must_use]
    pub fn resolve_lenient(&self, tree: &crate::Node) -> (crate::Node, Vec<String>) {
        let missing = self.missing(tree);
        // Resolves what it can and leaves the rest referenced. Returning the
        // original tree on any miss -- the obvious first implementation -- throws
        // away every asset that *was* found, which is the opposite of what
        // "lenient" should mean and makes it useless for a preflight.
        // In lenient mode an unresolved name is never an error, so this cannot
        // fail; the fallback is a clone rather than a silent drop.
        let resolved = resolve_node(tree, self, false).unwrap_or_else(|_| tree.clone());
        (resolved, missing)
    }

    /// A one-line summary, for logging or a test assertion.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} asset(s), {} bytes: {}",
            self.len(),
            self.total_bytes(),
            self.names().join(", ")
        )
    }
}

/// Walk a tree, handing every image reference to `f`.
///
/// A reference is a `src:` on an image node. Text and containers have no image of
/// their own, so they are skipped without a special case.
fn collect_refs(node: &crate::Node, f: &mut impl FnMut(&str)) {
    match node {
        crate::Node::Image { src, bytes, .. } => {
            // Only a node that was declared with a name and has not been
            // resolved counts as a reference. Once `bytes` is populated the node
            // stands on its own and needs nothing fetched.
            if bytes.is_empty() {
                if let Some(name) = src.as_deref() {
                    f(name);
                }
            }
        }
        crate::Node::Container { children, .. } => {
            for c in children {
                collect_refs(c, f);
            }
        }
        crate::Node::Text { .. } => {}
    }
}

/// Fill `src` references throughout a tree.
///
/// `strict` decides what an unresolved name does: return it as the error so
/// [`Assets::resolve`] can report it, or leave the node referenced so
/// [`Assets::resolve_lenient`] keeps everything else it did resolve. The error
/// carries the *name* rather than a formatted message so the caller can point at
/// the right asset.
fn resolve_node(node: &crate::Node, assets: &Assets, strict: bool) -> Result<crate::Node, String> {
    use crate::Node;
    Ok(match node {
        Node::Image {
            bytes,
            src,
            fit,
            style,
        } => {
            if !bytes.is_empty() {
                // Already inline: nothing to resolve, and nothing to complain
                // about.
                Node::Image {
                    bytes: bytes.clone(),
                    src: src.clone(),
                    fit: *fit,
                    style: style.clone(),
                }
            } else if let Some(name) = src {
                match assets.get(name) {
                    Some(resolved) => Node::Image {
                        bytes: resolved.to_vec(),
                        src: Some(name.clone()),
                        fit: *fit,
                        style: style.clone(),
                    },
                    None if strict => return Err(name.clone()),
                    None => Node::Image {
                        // Left referenced. The node renders as nothing, which is
                        // why `resolve` refuses to hand this to a renderer and
                        // `resolve_lenient` reports the name.
                        bytes: Vec::new(),
                        src: Some(name.clone()),
                        fit: *fit,
                        style: style.clone(),
                    },
                }
            } else {
                // No bytes and no name: an image node with nothing to draw. Left
                // as-is rather than rejected, because it is a zero-sized
                // placeholder some callers build deliberately.
                Node::Image {
                    bytes: Vec::new(),
                    src: None,
                    fit: *fit,
                    style: style.clone(),
                }
            }
        }
        Node::Container { style, children } => {
            let mut out = Vec::with_capacity(children.len());
            for c in children {
                out.push(resolve_node(c, assets, strict)?);
            }
            Node::Container {
                style: style.clone(),
                children: out,
            }
        }
        Node::Text { text, style } => Node::Text {
            text: text.clone(),
            style: style.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, Style};

    fn image(src: &str) -> Node {
        Node::image_ref(src, Style::new().with_size(10.0, 10.0))
    }

    #[test]
    fn an_empty_set_resolves_nothing() {
        let a = Assets::new();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert_eq!(a.total_bytes(), 0);
        assert!(a.get("anything").is_none());
        assert!(a.summary().contains("0 asset"));
    }

    #[test]
    fn bytes_round_trip_by_name() {
        let a = Assets::new().with("logo", vec![1u8, 2, 3]);
        assert_eq!(a.get("logo"), Some(&[1u8, 2, 3][..]));
        assert!(a.contains("logo"));
        assert!(!a.contains("other"));
        assert_eq!(a.len(), 1);
        assert_eq!(a.total_bytes(), 3);
    }

    #[test]
    fn insert_returns_the_length_so_a_caller_can_log_its_own_fetches() {
        let mut a = Assets::new();
        assert_eq!(
            a.insert("logo", "https://example.com/logo.png", vec![7u8; 10]),
            10
        );
        assert_eq!(a.total_bytes(), 10);
    }

    #[test]
    fn names_are_sorted_so_iteration_is_deterministic() {
        // A `HashMap` would make this order vary run to run, which is exactly the
        // kind of thing that makes a "deterministic" render stop being one.
        let a = Assets::new()
            .with("zebra", vec![1u8])
            .with("apple", vec![2u8])
            .with("mango", vec![3u8]);
        assert_eq!(a.names(), vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn inserting_the_same_name_twice_replaces_it() {
        let mut a = Assets::new();
        a.insert("logo", "u1", vec![1u8, 1, 1]);
        a.insert("logo", "u2", vec![2u8]);
        assert_eq!(a.len(), 1, "a second insert should replace, not accumulate");
        assert_eq!(a.get("logo"), Some(&[2u8][..]));
    }

    #[test]
    fn missing_lists_referenced_but_unresolved_assets() {
        let a = Assets::new().with("present", vec![1u8]);
        let tree = Node::container(
            Style::new(),
            vec![image("present"), image("absent"), image("also-absent")],
        );
        assert_eq!(a.missing(&tree), vec!["absent", "also-absent"]);
    }

    #[test]
    fn missing_deduplicates() {
        // Two nodes referencing one absent name is one thing to go and fetch.
        let a = Assets::new();
        let tree = Node::container(Style::new(), vec![image("x"), image("x"), image("x")]);
        assert_eq!(a.missing(&tree), vec!["x"]);
    }

    #[test]
    fn missing_finds_nested_references() {
        let a = Assets::new();
        let tree = Node::container(
            Style::new(),
            vec![Node::container(
                Style::new(),
                vec![Node::container(Style::new(), vec![image("deep")])],
            )],
        );
        assert_eq!(a.missing(&tree), vec!["deep"]);
    }

    #[test]
    fn a_fully_resolved_tree_is_missing_nothing() {
        let a = Assets::new().with("a", vec![1u8]).with("b", vec![2u8]);
        let tree = Node::container(Style::new(), vec![image("a"), image("b")]);
        assert!(a.missing(&tree).is_empty());
    }

    #[test]
    fn text_and_containers_are_not_references() {
        let a = Assets::new();
        let tree = Node::container(
            Style::new(),
            vec![Node::text("hello", Style::text(12.0, "#000000"))],
        );
        assert!(
            a.missing(&tree).is_empty(),
            "a text node must not look like an asset"
        );
    }
}
#[cfg(test)]
mod resolve_tests {
    use super::*;
    use crate::{Node, Style};

    fn page(children: Vec<Node>) -> Node {
        Node::container(Style::new().with_size(100.0, 100.0), children)
    }

    #[test]
    fn a_resolved_reference_becomes_real_bytes() {
        let a = Assets::new().with("logo", vec![1u8, 2, 3, 4]);
        let tree = page(vec![Node::image_ref("logo", Style::new())]);
        let out = a.resolve(&tree).expect("resolves");
        match &out {
            Node::Container { children, .. } => match &children[0] {
                Node::Image { bytes, .. } => assert_eq!(bytes, &[1, 2, 3, 4]),
                other => panic!("expected an image, got {other:?}"),
            },
            other => panic!("expected a container, got {other:?}"),
        }
    }

    #[test]
    fn an_unresolved_reference_is_an_error_not_a_blank_box() {
        // The failure this design exists to prevent: a missing image rendering as
        // an empty box, which looks identical to a transparent one.
        let a = Assets::new();
        let tree = page(vec![Node::image_ref("nope", Style::new())]);
        let err = a.resolve(&tree).expect_err("must fail");
        assert!(
            format!("{err:?}").contains("nope"),
            "the error should name the asset, got {err:?}"
        );
    }

    #[test]
    fn resolution_recurses_through_containers() {
        let a = Assets::new().with("deep", vec![9u8]);
        let tree = page(vec![Node::container(
            Style::new(),
            vec![Node::container(
                Style::new(),
                vec![Node::image_ref("deep", Style::new())],
            )],
        )]);
        let out = a.resolve(&tree).expect("resolves");
        let bytes = find_first_image(&out).expect("image survived");
        assert_eq!(bytes, &[9u8]);
    }

    #[test]
    fn an_inline_image_is_left_alone() {
        // A tree that already has bytes needs no assets at all.
        let a = Assets::new();
        let tree = page(vec![Node::image(vec![7u8, 7, 7], Style::new())]);
        let out = a.resolve(&tree).expect("resolves with no assets");
        assert_eq!(
            find_first_image(&out).expect("image"),
            &[7u8, 7, 7],
            "inline bytes must be passed through untouched"
        );
    }

    #[test]
    fn a_resolved_node_is_not_still_a_reference() {
        // After resolution the node stands on its own, so a second pass over the
        // same assets must find nothing left to do.
        let a = Assets::new().with("logo", vec![1u8]);
        let tree = page(vec![Node::image_ref("logo", Style::new())]);
        let once = a.resolve(&tree).expect("first pass");
        assert!(
            a.missing(&once).is_empty(),
            "a resolved tree should report nothing missing: {:?}",
            a.missing(&once)
        );
    }

    #[test]
    fn resolving_is_idempotent() {
        let a = Assets::new().with("logo", vec![1u8, 2]);
        let tree = page(vec![Node::image_ref("logo", Style::new())]);
        let once = a.resolve(&tree).expect("first");
        let twice = a.resolve(&once).expect("second");
        assert_eq!(once, twice, "resolving twice changed the tree");
    }

    #[test]
    fn resolve_lenient_lists_everything_missing_at_once() {
        let a = Assets::new().with("present", vec![1u8]);
        let tree = page(vec![
            Node::image_ref("present", Style::new()),
            Node::image_ref("gone-a", Style::new()),
            Node::image_ref("gone-b", Style::new()),
        ]);
        let (out, missing) = a.resolve_lenient(&tree);
        assert_eq!(missing, vec!["gone-a", "gone-b"]);
        // The present one still resolved, so the partial tree is usable.
        assert!(out != tree);
    }

    #[test]
    fn an_empty_resolved_tree_equals_the_original() {
        let a = Assets::new();
        let tree = page(vec![Node::text("x", Style::text(10.0, "#000000"))]);
        assert_eq!(a.resolve(&tree).expect("resolves"), tree);
    }

    fn find_first_image(node: &Node) -> Option<&[u8]> {
        match node {
            Node::Image { bytes, .. } if !bytes.is_empty() => Some(bytes),
            Node::Image { .. } => None,
            Node::Container { children, .. } => children.iter().find_map(find_first_image),
            Node::Text { .. } => None,
        }
    }
}
