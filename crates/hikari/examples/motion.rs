//! motion.rs target: 3-frame animated GIF + APNG demo.

mod common;

use hikari::{render_animation_apng, render_animation_gif, Node};

fn main() {
    let bgs = ["#0b1020", "#1e1b4b", "#082f22"];
    let frames: Vec<(Node, u32)> = bgs
        .iter()
        .enumerate()
        .map(|(i, bg)| {
            (
                Node::banner(
                    480.0,
                    240.0,
                    bg,
                    &format!("Hikari Motion {}", i + 1),
                    48.0,
                    "#ffffff",
                ),
                600,
            )
        })
        .collect();
    let gif = render_animation_gif(&frames, 480, 240).expect("render");
    common::write("motion.gif", &gif);
    println!("wrote motion.gif ({} bytes)", gif.len());
    let apng = render_animation_apng(&frames, 480, 240).expect("render");
    common::write("motion.apng.png", &apng);
    println!("wrote motion.apng.png ({} bytes)", apng.len());
}
