//! The filter colour matrix, checked against hand-computed values.
//!
//! These are arithmetic tests, not rendering tests. The matrix is folded from
//! six independent operations into one affine transform, and the composition
//! rule has a term (`m[4] * sum(w) + offset`) that is easy to get wrong in a way
//! that only shows up once two operations are combined. Asserting the folded
//! result against values worked out by hand catches that immediately, where a
//! render test would just produce a slightly wrong colour and pass the ink
//! floor.
//!
//! `Filter::color_matrix` is `pub` precisely so this can be tested from here.

use super::{Filter, MaskMode, MaskSource, Style, StyleMask};

/// Apply the matrix to a colour, returning the clamped 0..=255 result.
fn apply(m: &[[f32; 5]; 4], r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let c = [r as f32, g as f32, b as f32];
    let mut out = [0u8; 3];
    for row in 0..3 {
        let v = m[row][0] * c[0] + m[row][1] * c[1] + m[row][2] * c[2] + m[row][4];
        out[row] = v.round().clamp(0.0, 255.0) as u8;
    }
    (out[0], out[1], out[2])
}

#[test]
fn no_colour_operation_yields_no_matrix() {
    // `None` must mean "skip the pass entirely", not "build an identity matrix
    // and run it". Building one would cost a full-frame pass for nothing.
    let f = Filter::default();
    assert!(
        f.color_matrix().is_none(),
        "default filter should have no matrix"
    );
    assert!(f.is_identity(), "a default filter must be a no-op");
}

#[test]
fn brightness_scales_each_channel() {
    let m = Filter {
        brightness: Some(2.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(apply(&m, 10, 20, 30), (20, 40, 60));
    // 200 * 2 clips rather than wrapping, which is CSS `clamp` behaviour.
    assert_eq!(apply(&m, 200, 200, 200), (255, 255, 255));
}

#[test]
fn contrast_pivots_about_mid_grey() {
    // CSS: out = c*f + 0.5*(1-f). At f=0 everything collapses to mid grey; at
    // f=1 it is identity. Getting the pivot wrong shifts the whole image rather
    // than distorting it, so it is checked at both ends.
    let zero = Filter {
        contrast: Some(0.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(apply(&zero, 0, 128, 255), (128, 128, 128));

    let one = Filter {
        contrast: Some(1.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(apply(&one, 10, 20, 30), (10, 20, 30));

    let half = Filter {
        contrast: Some(0.5),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    // out = c*0.5 + 0.5*0.5 = c*0.5 + 0.25, in 0..=255 units.
    //   0   -> 63.75  -> 64
    //   255 -> 191.25 -> 191
    assert_eq!(apply(&half, 0, 0, 0), (64, 64, 64));
    assert_eq!(apply(&half, 255, 255, 255), (191, 191, 191));
}

#[test]
fn grayscale_produces_equal_channels() {
    let m = Filter {
        grayscale: Some(1.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    // Rec.709 luma of pure red: 0.2126 * 255 = 54.2.
    assert_eq!(apply(&m, 255, 0, 0), (54, 54, 54));
    // Of pure green: 0.7152 * 255 = 182.4.
    assert_eq!(apply(&m, 0, 255, 0), (182, 182, 182));
}

#[test]
fn saturate_zero_is_the_same_as_full_grayscale() {
    // Both are "lerp toward luma", parameterised differently. If they disagree,
    // one of the two is wrong.
    let grey = Filter {
        grayscale: Some(1.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    let sat = Filter {
        saturate: Some(0.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    for px in [
        (255u8, 0u8, 0u8),
        (0u8, 255u8, 0u8),
        (0u8, 0u8, 255u8),
        (30u8, 60u8, 90u8),
    ] {
        assert_eq!(
            apply(&grey, px.0, px.1, px.2),
            apply(&sat, px.0, px.1, px.2),
            "disagreement at {px:?}"
        );
    }
}

#[test]
fn invert_is_an_identity_plus_a_flip() {
    let m = Filter {
        invert: Some(1.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(apply(&m, 0, 0, 0), (255, 255, 255));
    assert_eq!(apply(&m, 255, 255, 255), (0, 0, 0));

    let half = Filter {
        invert: Some(0.5),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(apply(&half, 0, 0, 0), (128, 128, 128));
}

#[test]
fn sepia_warms_the_image() {
    let m = Filter {
        sepia: Some(1.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");
    // White through the standard sepia matrix clips: its rows sum to 1.351,
    // 1.203 and 0.937, so red saturates. Testing on white would assert clipping
    // behaviour rather than the tint, so this uses mid grey, where nothing
    // clips and the warmth is the thing being measured.
    let (r, g, b) = apply(&m, 128, 128, 128);
    assert!(
        r > g && g > b,
        "sepia should order red > green > blue, got {r},{g},{b}"
    );
    assert!(b < 128, "sepia should pull blue down, got {b}");
}

#[test]
fn operations_compose_in_the_documented_order() {
    // Two colour operations folded together must equal applying them by hand in
    // the documented order. This is the test that catches a wrong offset term:
    // the single-operation cases all pass with `m[4] = offset`, because nothing
    // before them has contributed an offset to preserve.
    let m = Filter {
        contrast: Some(0.5),
        brightness: Some(2.0),
        ..Filter::default()
    }
    .color_matrix()
    .expect("matrix");

    // Hand: contrast first, then brightness.
    //   c1 = c*0.5 + 0.5*0.5
    //   c2 = c1*2
    // so c2 = c + 0.5, i.e. black becomes mid grey.
    for px in [(0u8, 0u8, 0u8), (128u8, 128u8, 128u8), (200u8, 100u8, 50u8)] {
        let mut hand = [0u8; 3];
        for (i, v) in [px.0, px.1, px.2].into_iter().enumerate() {
            let c = v as f32;
            let after_contrast = c * 0.5 + 0.5 * 255.0 * 0.5;
            hand[i] = (after_contrast * 2.0).round().clamp(0.0, 255.0) as u8;
        }
        assert_eq!(
            apply(&m, px.0, px.1, px.2),
            (hand[0], hand[1], hand[2]),
            "composition differs for {px:?}"
        );
    }
}

#[test]
fn alpha_row_is_never_touched_by_a_colour_operation() {
    // Alpha is applied separately and after the matrix. If a colour operation
    // ever wrote to row 3, a colour tweak would quietly become an opacity
    // change -- which is the kind of bug that survives every ink-count check.
    let m = Filter {
        opacity: None,
        grayscale: Some(1.0),
        sepia: Some(0.8),
        invert: Some(0.3),
        contrast: Some(1.4),
        brightness: Some(1.1),
        saturate: Some(0.6),
    }
    .color_matrix()
    .expect("matrix");
    assert_eq!(m[3], [0.0, 0.0, 0.0, 0.0, 0.0], "alpha row was modified");
}

#[test]
fn identity_values_are_recognised_as_such() {
    // Every field at its identity value must report `is_identity`, or a caller
    // writing `Filter { brightness: Some(1.0), ..Default::default() }` would pay
    // for an offscreen pass that changes nothing -- and would round its pixels
    // through a composite on the way.
    for f in [
        Filter {
            brightness: Some(1.0),
            ..Filter::default()
        },
        Filter {
            contrast: Some(1.0),
            ..Filter::default()
        },
        Filter {
            saturate: Some(1.0),
            ..Filter::default()
        },
        Filter {
            grayscale: Some(0.0),
            ..Filter::default()
        },
        Filter {
            sepia: Some(0.0),
            ..Filter::default()
        },
        Filter {
            invert: Some(0.0),
            ..Filter::default()
        },
        Filter {
            opacity: Some(1.0),
            ..Filter::default()
        },
    ] {
        assert!(f.is_identity(), "{f:?} should be identity");
        assert!(
            f.color_matrix().is_none_or(|m| near_identity(&m)),
            "{f:?} produced a non-identity matrix"
        );
    }

    assert!(
        !Filter {
            invert: Some(0.5),
            ..Filter::default()
        }
        .is_identity(),
        "a real invert must not report identity"
    );
}

/// Is this matrix within rounding distance of a colour identity?
fn near_identity(m: &[[f32; 5]; 4]) -> bool {
    let want = [
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0, 0.0],
    ];
    (0..3).all(|r| (0..5).all(|c| (m[r][c] - want[r][c]).abs() < 1e-6))
}

#[test]
fn filter_and_mask_survive_a_json_round_trip() {
    // The Node and WASM bindings round-trip trees as JSON, so anything on
    // `Style` has to survive that. A field that existed only in Rust would be
    // silently dropped by the bindings and render as "no filter at all".
    let style = Style {
        filter: Some(Filter {
            brightness: Some(1.2),
            ..Filter::default()
        }),
        mask: Some(StyleMask {
            source: MaskSource::SelfImage,
            mode: MaskMode::Luminance,
        }),
        ..Style::new()
    };
    let json = serde_json::to_string(&style).expect("serialize");
    let back: Style = serde_json::from_str(&json).expect("round-trip");
    assert_eq!(back.filter, style.filter, "filter did not survive JSON");
    assert_eq!(back.mask, style.mask, "mask did not survive JSON");
    assert!(
        json.contains("brightness"),
        "field name missing from {json}"
    );
    assert!(json.contains("luminance"), "mode missing from {json}");
}

#[test]
fn a_filter_that_is_absent_is_distinguishable_from_an_identity_one() {
    // `None` must mean "no filter", distinct from "a filter that does nothing".
    // Collapsing them would mean a caller could never turn a filter off, and a
    // tree that meant `Normal` could not be expressed.
    let absent: Option<Filter> = None;
    let real = Filter {
        grayscale: Some(1.0),
        ..Filter::default()
    };
    assert!(absent.is_none());
    assert!(absent.is_none_or(|f| f.is_identity()));
    assert!(
        !real.is_identity(),
        "a real filter must not report identity"
    );
    assert!(
        real.color_matrix().is_some(),
        "a real filter must produce a matrix"
    );
}
