//! invoice.rs target: flowing A4 Pro PDF demo (dev license).
//!
//! One flowing document node becomes N pages: line items split across
//! pages, the table header repeats, the title lands in the outline.

use hikari::{
    now_unix, render_pdf_with, Attachment, Justify, License, Node, PageSize, PdfOptions, Style,
};

fn row(cells: [&str; 4], header: bool) -> Node {
    let mut style = Style::grid(4).with_gap(8.0);
    if header {
        style = style.repeat_header();
    }
    Node::container(
        style,
        cells
            .iter()
            .map(|c| Node::text(c, Style::text(13.0, "#0f172a")))
            .collect(),
    )
}

fn item_row(i: usize) -> Node {
    let cells = [
        format!("Support block {i}"),
        "1".to_owned(),
        "$49.00".to_owned(),
        "$49.00".to_owned(),
    ];
    Node::container(
        Style::grid(4).with_gap(8.0),
        cells
            .iter()
            .map(|c| Node::text(c, Style::text(13.0, "#0f172a")))
            .collect(),
    )
}

fn document() -> Node {
    let mut kids = vec![
        Node::container(
            Style::column()
                .with_background("#0b1020")
                .with_radius(12.0)
                .with_justify(Justify::Center)
                .with_padding(16.0)
                .keep_together(),
            vec![
                Node::text(
                    "INVOICE #2026-091",
                    Style::text(28.0, "#ffffff").with_bookmark(1),
                ),
                Node::text(
                    "Hikari Labs — hello@hikari.example",
                    Style::text(14.0, "#94a3b8"),
                ),
            ],
        ),
        row(["Item", "Qty", "Price", "Total"], true),
    ];
    for i in 1..=30 {
        kids.push(item_row(i));
    }
    kids.push(Node::text(
        "Total due: $1,660.00 — thank you!",
        Style::text(18.0, "#0f172a")
            .keep_together()
            .with_bookmark(2),
    ));
    kids.push(Node::text(
        "Pay online: pay.hikari.example/i/2026-091",
        Style::text(14.0, "#2563eb").with_link("https://pay.hikari.example/i/2026-091"),
    ));
    Node::container(
        Style::column()
            .with_size(595.28, 841.89)
            .with_padding(40.0)
            .with_gap(12.0),
        kids,
    )
}

fn main() {
    let lic = License::dev(now_unix());
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<invoice id="2026-091" currency="USD">
  <seller>Hikari Labs</seller>
  <total>1660.00</total>
</invoice>"#;
    let options = PdfOptions {
        title: Some("Invoice 2026-091".into()),
        author: Some("Hikari Labs".into()),
        outline: true,
        paginate: true,
        attachments: vec![Attachment {
            name: "factur-x.xml".into(),
            mime: "text/xml".into(),
            bytes: xml.as_bytes().to_vec(),
        }],
    };
    let doc = render_pdf_with(&[document()], PageSize::A4, &options, &lic).expect("render");
    std::fs::write("/Users/apple/hikari/invoice.pdf", &doc).expect("write");
    println!("wrote invoice.pdf ({} bytes)", doc.len());
}
