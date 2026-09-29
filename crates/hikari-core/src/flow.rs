//! Flow pagination: split a column container's children across pages.
//!
//! v1 rules, stated openly:
//!
//! - Text splits by wrapped lines (unless `keep_together`).
//! - Every other box is atomic: it moves whole to the next page when it
//!   does not fit, and occupies a page alone (overflowing) when taller
//!   than one page.
//! - Children flagged `repeat_header` hoist to the top of every page
//!   (including the first) and repeat on continuations (table headers).
//! - Inter-child gaps come from the root style and are budgeted per page.

use crate::{compute_layout, line_height, wrap_text, Node, Style, BUILTIN_FONT};

/// Page box for flowing content.
#[derive(Debug, Clone, Copy)]
pub struct Flow {
    /// Page width in px.
    pub page_w: f32,
    /// Page height in px.
    pub page_h: f32,
    /// Inner padding applied by each page wrapper.
    pub padding: f32,
}

/// Split a column container into one page node per page.
///
/// Non-container roots pass through unchanged. Page wrappers reuse the
/// root style with the page size and [`Flow::padding`].
#[must_use]
pub fn paginate(root: &Node, flow: &Flow) -> Vec<Node> {
    let (style, children) = match root {
        Node::Container { style, children } => (style.clone(), children.clone()),
        _ => return vec![root.clone()],
    };
    let content_w = (flow.page_w - 2.0 * flow.padding).max(1.0);
    let content_h = (flow.page_h - 2.0 * flow.padding).max(1.0);
    let gap = style.gap.max(0.0);
    let header: Vec<Node> = children.iter().filter(|c| is_header(c)).cloned().collect();
    let body: Vec<Node> = children.into_iter().filter(|c| !is_header(c)).collect();
    let header_h: f32 = header.iter().map(|h| probe_height(h, content_w)).sum();

    let mut pages: Vec<Vec<Node>> = vec![header.clone()];
    // Remaining content height on the current page (headers hoist to page 1 too).
    let mut remaining = content_h - header_h;
    let mut first_on_page = header.is_empty();

    let mut idx = 0;
    while idx < body.len() {
        let child = body[idx].clone();
        match split_text(&child, content_w) {
            // Splittable text: fill by lines.
            Some((fs, lines)) if !child_style(&child).keep_together => {
                let lh = line_height(fs);
                let mut lines = lines;
                while !lines.is_empty() {
                    if !first_on_page {
                        remaining -= gap;
                    }
                    let mut k = (remaining / lh).floor() as usize;
                    if k == 0 {
                        if remaining < content_h - header_h_for(&pages, header_h) {
                            new_page(&mut pages, &header, &mut remaining, content_h, header_h);
                            first_on_page = header.is_empty();
                            continue;
                        }
                        // Page shorter than one line: emit one line (overflow).
                        k = 1;
                    }
                    k = k.min(lines.len());
                    pages.last_mut().expect("page").push(text_fragment(
                        &child,
                        lines[..k].join("\n"),
                        content_w,
                    ));
                    remaining -= k as f32 * lh;
                    first_on_page = false;
                    lines = lines[k..].to_vec();
                    if !lines.is_empty() {
                        new_page(&mut pages, &header, &mut remaining, content_h, header_h);
                        first_on_page = header.is_empty();
                    }
                }
                idx += 1;
            }
            // Atomic box (or keep-together text): move whole on overflow.
            _ => {
                let h = probe_height(&child, content_w);
                if !first_on_page {
                    remaining -= gap;
                }
                if h > remaining && !first_on_page {
                    new_page(&mut pages, &header, &mut remaining, content_h, header_h);
                    if !header.is_empty() {
                        remaining -= gap;
                    }
                }
                // Taller than a page: occupies one alone (overflow, documented).
                pages.last_mut().expect("page").push(child);
                remaining -= h.min(remaining);
                first_on_page = false;
                idx += 1;
            }
        }
    }

    pages
        .into_iter()
        .map(|kids| {
            Node::container(
                Style {
                    width: Some(flow.page_w),
                    height: Some(flow.page_h),
                    padding: flow.padding,
                    ..style.clone()
                },
                kids,
            )
        })
        .collect()
}

fn header_h_for(pages: &[Vec<Node>], header_h: f32) -> f32 {
    if pages.len() > 1 {
        header_h
    } else {
        0.0
    }
}

fn new_page(
    pages: &mut Vec<Vec<Node>>,
    header: &[Node],
    remaining: &mut f32,
    content_h: f32,
    header_h: f32,
) {
    pages.push(header.to_vec());
    *remaining = content_h - header_h;
}

fn is_header(node: &Node) -> bool {
    match node {
        Node::Container { style, .. } | Node::Text { style, .. } | Node::Image { style, .. } => {
            style.repeat_header
        }
    }
}

fn child_style(node: &Node) -> Style {
    match node {
        Node::Container { style, .. } | Node::Text { style, .. } | Node::Image { style, .. } => {
            style.clone()
        }
    }
}

/// If `node` is text, return `(font_size, wrapped_lines)` at content width.
fn split_text(node: &Node, content_w: f32) -> Option<(f32, Vec<String>)> {
    match node {
        Node::Text { text, style } => {
            let fs = style.font_size.unwrap_or(16.0).max(1.0);
            let width = style.max_width.unwrap_or(content_w).max(1.0);
            Some((
                fs,
                wrap_text(text, fs, width, style.font.unwrap_or(BUILTIN_FONT))
                    .split('\n')
                    .map(str::to_owned)
                    .collect(),
            ))
        }
        _ => None,
    }
}

/// Text fragment that keeps wrapping identically inside the page layout.
fn text_fragment(node: &Node, text: String, content_w: f32) -> Node {
    match node {
        Node::Text { style, .. } => Node::Text {
            text,
            style: Style {
                max_width: Some(style.max_width.unwrap_or(content_w)),
                ..style.clone()
            },
        },
        other => other.clone(),
    }
}

/// Natural height of a child at `content_w` via a probe layout.
fn probe_height(node: &Node, content_w: f32) -> f32 {
    let probe = Node::container(
        Style::column().with_size(content_w, 1_000_000.0),
        vec![node.clone()],
    );
    compute_layout(&probe, content_w, 1_000_000.0)
        .map(|p| p.children.first().map(|c| c.h).unwrap_or(0.0))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Node {
        Node::container(
            Style::column().with_gap(0.0),
            (0..n)
                .map(|i| Node::text(&format!("row {i}"), Style::text(20.0, "#000")))
                .collect(),
        )
    }

    #[test]
    fn splits_rows_preserving_order() {
        let root = rows(20);
        // ~25px per row at 20px font; 200px content fits ~8 rows.
        let pages = paginate(
            &root,
            &Flow {
                page_w: 400.0,
                page_h: 200.0,
                padding: 0.0,
            },
        );
        assert!(pages.len() >= 2, "pages={}", pages.len());
        let texts: Vec<String> = pages
            .iter()
            .flat_map(|p| match p {
                Node::Container { children, .. } => children
                    .iter()
                    .filter_map(|c| match c {
                        Node::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect();
        let expect: Vec<String> = (0..20).map(|i| format!("row {i}")).collect();
        assert_eq!(texts, expect);
    }

    #[test]
    fn keep_together_moves_whole() {
        let block = || {
            Node::container(
                Style::column(),
                vec![
                    Node::text("head", Style::text(20.0, "#000")),
                    Node::text("tail", Style::text(20.0, "#000")),
                ],
            )
        };
        let mut a = block();
        if let Node::Container { style, .. } = &mut a {
            *style = style.clone().keep_together();
        }
        let root = Node::container(Style::column().with_gap(0.0), vec![a, block()]);
        let pages = paginate(
            &root,
            &Flow {
                page_w: 400.0,
                page_h: 90.0,
                padding: 0.0,
            },
        );
        // Each 2-line block (~50px) must stay intact on one page.
        assert_eq!(pages.len(), 2);
        for p in &pages {
            let texts = page_texts(p);
            assert_eq!(texts, vec!["head".to_owned(), "tail".to_owned()]);
        }
    }

    /// All text leaves in a page, in order.
    fn page_texts(page: &Node) -> Vec<String> {
        fn walk(n: &Node, out: &mut Vec<String>) {
            match n {
                Node::Text { text, .. } => out.push(text.clone()),
                Node::Container { children, .. } => children.iter().for_each(|c| walk(c, out)),
                Node::Image { .. } => {}
            }
        }
        let mut out = Vec::new();
        walk(page, &mut out);
        out
    }

    #[test]
    fn header_repeats_every_page() {
        let root = Node::container(
            Style::column().with_gap(0.0),
            std::iter::once(Node::text(
                "HEADER",
                Style::text(20.0, "#000").repeat_header(),
            ))
            .chain((0..20).map(|i| Node::text(&format!("row {i}"), Style::text(20.0, "#000"))))
            .collect(),
        );
        let pages = paginate(
            &root,
            &Flow {
                page_w: 400.0,
                page_h: 200.0,
                padding: 0.0,
            },
        );
        assert!(pages.len() >= 2);
        for p in pages.iter().skip(1) {
            let first = match p {
                Node::Container { children, .. } => match &children[0] {
                    Node::Text { text, .. } => text.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            assert_eq!(first, "HEADER");
        }
    }

    #[test]
    fn tall_box_overflows_alone() {
        let tall = Node::container(
            Style::column().with_size(100.0, 500.0),
            vec![Node::text("big", Style::text(20.0, "#000"))],
        );
        let root = Node::container(Style::column().with_gap(0.0), vec![tall]);
        let pages = paginate(
            &root,
            &Flow {
                page_w: 400.0,
                page_h: 200.0,
                padding: 0.0,
            },
        );
        assert_eq!(pages.len(), 1);
    }
}
