#[cfg(test)]
mod tests {
    use float_cmp::ApproxEq;
    use rustdct::DctPlanner;

    #[test]
    fn dct_16_x_16() {
        let dim: usize = 16;
        let divisor = 4.0 / (dim as f64 * dim as f64);
        let mut arr = vec![0.0; dim * dim];

        for i in 0..dim {
            for j in 0..dim {
                for ii in i..dim {
                    for jj in j..dim {
                        arr[ii * dim + jj] += 1.0;
                    }
                }
            }
        }
        for elem in arr.iter_mut() {
            *elem -= 256.0 / 2.0;
        }
        let orig = arr.clone();

        let mut planner = DctPlanner::new();
        let dct = planner.plan_dct2(dim as usize);

        //// Perform forward DCT
        arr.chunks_exact_mut(dim as usize).for_each(|row| {
            println!("{:?}", row);
            dct.process_dct2(row);
            println!("{:?}", row);
        });

        let mut transpose_buffer = vec![0.0; dim];
        transpose::transpose_inplace(&mut arr, &mut transpose_buffer, dim as usize, dim as usize);

        arr.chunks_exact_mut(dim as usize).for_each(|row| {
            println!("{:?}", row);
            dct.process_dct2(row);
            println!("{:?}", row);
        });
        transpose::transpose_inplace(&mut arr, &mut transpose_buffer, dim as usize, dim as usize);

        // scale the coefficients
        for elem in arr.iter_mut() {
            *elem = *elem * divisor;
        }
        //// End forward DCT

        //// Perform inverse DCT
        arr.chunks_exact_mut(dim as usize).for_each(|row| {
            println!("{:?}", row);
            dct.process_dct3(row);
            println!("{:?}", row);
        });
        transpose::transpose_inplace(&mut arr, &mut transpose_buffer, dim as usize, dim as usize);

        arr.chunks_exact_mut(dim as usize).for_each(|row| {
            println!("{:?}", row);
            dct.process_dct3(row);
            println!("{:?}", row);
        });
        transpose::transpose_inplace(&mut arr, &mut transpose_buffer, dim as usize, dim as usize);
        //// End inverse DCT

        // Check that the original array is equal to the reconstructed array
        for (new, old) in arr.iter().zip(orig.iter()) {
            assert!(new.approx_eq(*old, (1.0e-9, 5)));
        }
    }
}
