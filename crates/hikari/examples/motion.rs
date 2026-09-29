//! motion.rs target: 3-frame Pro GIF + APNG demo (dev license).

use hikari::{now_unix, render_animation_apng, render_animation_gif, License, Node};

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
    let lic = License::dev(now_unix());
    let gif = render_animation_gif(&frames, 480, 240, &lic).expect("render");
    std::fs::write("/Users/apple/hikari/motion.gif", &gif).expect("write");
    println!("wrote motion.gif ({} bytes)", gif.len());
    let apng = render_animation_apng(&frames, 480, 240, &lic).expect("render");
    std::fs::write("/Users/apple/hikari/motion.apng.png", &apng).expect("write");
    println!("wrote motion.apng.png ({} bytes)", apng.len());
}
