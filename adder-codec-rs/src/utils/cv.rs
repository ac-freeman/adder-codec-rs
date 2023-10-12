use adder_codec_core::{Coord, Event, PlaneSize};
use ndarray::Array3;
use std::error::Error;

// TODO: Explore optimal threshold values
pub const INTENSITY_THRESHOLD: i32 = 30;

/// Indices for the asynchronous FAST 9_16 algorithm
#[rustfmt::skip]
const CIRCLE3: [[i32; 2]; 16] = [
    [0, 3], [1, 3], [2, 2], [3, 1],
    [3, 0], [3, -1], [2, -2], [1, -3],
    [0, -3], [-1, -3], [-2, -2], [-3, -1],
    [-3, 0], [-3, 1], [-2, 2], [-1, 3]
];

/// Check if the given event is a feature
pub fn is_feature(
    coord: Coord,
    plane: PlaneSize,
    img: &Array3<i32>,
) -> Result<bool, Box<dyn Error>> {
    if coord.is_border(plane.w_usize(), plane.h_usize(), 3) {
        return Ok(false);
    }
    unsafe {
        let candidate: i32 = *img.uget((coord.y_usize(), coord.x_usize(), 0));
        let y = coord.y as i32;
        let x = coord.x as i32;

        let mut count = 0;
        if (*img.uget((
            (y + CIRCLE3[4][1]) as usize,
            (x + CIRCLE3[4][0]) as usize,
            0,
        )) - candidate)
            .abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }
        if (*img.uget((
            (y + CIRCLE3[12][1]) as usize,
            (x + CIRCLE3[12][0]) as usize,
            0,
        )) - candidate)
            .abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }
        if (*img.uget((
            (y + CIRCLE3[1][1]) as usize,
            (x + CIRCLE3[1][0]) as usize,
            0,
        )) - candidate)
            .abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }

        if (*img.uget((
            (y + CIRCLE3[7][1]) as usize,
            (x + CIRCLE3[7][0]) as usize,
            0,
        )) - candidate)
            .abs()
            > INTENSITY_THRESHOLD
        {
            count += 1;
        }

        if count <= 2 {
            return Ok(false);
        }

        let streak_size = 12;

        for i in 0..16 {
            // Are we looking at a bright or dark streak?
            let brighter = *img.uget((
                (y + CIRCLE3[i][1]) as usize,
                (x + CIRCLE3[i][0]) as usize,
                0,
            )) > candidate;

            let mut did_break = false;

            for j in 0..streak_size {
                if brighter {
                    if *img.uget((
                        (y + CIRCLE3[(i + j) % 16][1]) as usize,
                        (x + CIRCLE3[(i + j) % 16][0]) as usize,
                        0,
                    )) <= candidate + INTENSITY_THRESHOLD
                    {
                        did_break = true;
                    }
                } else if *img.uget((
                    (y + CIRCLE3[(i + j) % 16][1]) as usize,
                    (x + CIRCLE3[(i + j) % 16][0]) as usize,
                    0,
                )) >= candidate - INTENSITY_THRESHOLD
                {
                    did_break = true;
                }
            }

            if !did_break {
                return Ok(true);
            }
        }
    }

    Ok(false)
}
