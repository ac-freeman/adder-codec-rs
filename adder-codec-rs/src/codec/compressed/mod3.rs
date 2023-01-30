use crate::codec::compressed::BLOCK_SIZE;
use crate::framer::driver::EventCoordless;
use crate::{DeltaT, D};
use bitvec::bitvec;
use bitvec::field::BitField;
use bitvec::prelude::{BitVec, Msb0};

// pub struct BlockItem<'a> {
//     event: EventCoordless,
//     node_ref: BasePixel<'a>,
// }

pub type Block2 = [Option<EventCoordless>; BLOCK_SIZE * BLOCK_SIZE];

pub type BlockTreeReferences<'a> = [&'a Option<EventCoordless>; BLOCK_SIZE * BLOCK_SIZE];

fn block_tree_references<'a>(b: &'a Block2) -> BlockTreeReferences<'a> {
    let mut r = [&None; BLOCK_SIZE * BLOCK_SIZE];
    r[0] = &b[0];
    r[1] = &b[1];
    r[2] = &b[BLOCK_SIZE * 1 + 0];
    r[3] = &b[BLOCK_SIZE * 1 + 1];
    r[4] = &b[2];
    r[5] = &b[3];
    r[6] = &b[BLOCK_SIZE * 1 + 2];
    r[7] = &b[BLOCK_SIZE * 1 + 3];
    r[8] = &b[BLOCK_SIZE * 2 + 0];
    r[9] = &b[BLOCK_SIZE * 2 + 1];
    r[10] = &b[BLOCK_SIZE * 3 + 0];
    r[11] = &b[BLOCK_SIZE * 3 + 1];
    r[12] = &b[BLOCK_SIZE * 2 + 2];
    r[13] = &b[BLOCK_SIZE * 2 + 3];
    r[14] = &b[BLOCK_SIZE * 3 + 2];
    r[15] = &b[BLOCK_SIZE * 3 + 3];
    // Done with top-left 4x4 block

    r[16] = &b[4];
    r[17] = &b[5];
    r[18] = &b[BLOCK_SIZE * 1 + 4];
    r[19] = &b[BLOCK_SIZE * 1 + 5];
    r[20] = &b[6];
    r[21] = &b[7];
    r[22] = &b[BLOCK_SIZE * 1 + 6];
    r[23] = &b[BLOCK_SIZE * 1 + 7];
    r[24] = &b[BLOCK_SIZE * 2 + 4];
    r[25] = &b[BLOCK_SIZE * 2 + 5];
    r[26] = &b[BLOCK_SIZE * 3 + 4];
    r[27] = &b[BLOCK_SIZE * 3 + 5];
    r[28] = &b[BLOCK_SIZE * 2 + 6];
    r[29] = &b[BLOCK_SIZE * 2 + 7];
    r[30] = &b[BLOCK_SIZE * 3 + 6];
    r[31] = &b[BLOCK_SIZE * 3 + 7];
    // Done with top-left --> right 1 4x4 block

    r[32] = &b[BLOCK_SIZE * 4 + 0];
    r[33] = &b[BLOCK_SIZE * 4 + 1];
    r[34] = &b[BLOCK_SIZE * 5 + 0];
    r[35] = &b[BLOCK_SIZE * 5 + 1];
    r[36] = &b[BLOCK_SIZE * 4 + 2];
    r[37] = &b[BLOCK_SIZE * 4 + 3];
    r[38] = &b[BLOCK_SIZE * 5 + 2];
    r[39] = &b[BLOCK_SIZE * 5 + 3];
    r[40] = &b[BLOCK_SIZE * 6 + 0];
    r[41] = &b[BLOCK_SIZE * 6 + 1];
    r[42] = &b[BLOCK_SIZE * 7 + 0];
    r[43] = &b[BLOCK_SIZE * 7 + 1];
    r[44] = &b[BLOCK_SIZE * 6 + 2];
    r[45] = &b[BLOCK_SIZE * 6 + 3];
    r[46] = &b[BLOCK_SIZE * 7 + 2];
    r[47] = &b[BLOCK_SIZE * 7 + 3];
    // Done with top-left --> down 1 4x4 block

    r[48] = &b[BLOCK_SIZE * 4 + 4];
    r[49] = &b[BLOCK_SIZE * 4 + 5];
    r[50] = &b[BLOCK_SIZE * 5 + 4];
    r[51] = &b[BLOCK_SIZE * 5 + 5];
    r[52] = &b[BLOCK_SIZE * 4 + 6];
    r[53] = &b[BLOCK_SIZE * 4 + 7];
    r[54] = &b[BLOCK_SIZE * 5 + 6];
    r[55] = &b[BLOCK_SIZE * 5 + 7];
    r[56] = &b[BLOCK_SIZE * 6 + 4];
    r[57] = &b[BLOCK_SIZE * 6 + 5];
    r[58] = &b[BLOCK_SIZE * 7 + 4];
    r[59] = &b[BLOCK_SIZE * 7 + 5];
    r[60] = &b[BLOCK_SIZE * 6 + 6];
    r[61] = &b[BLOCK_SIZE * 6 + 7];
    r[62] = &b[BLOCK_SIZE * 7 + 6];
    r[63] = &b[BLOCK_SIZE * 7 + 7];
    // Done with top-left --> down 1 --> right 1 4x4 block
    // Done with first 8x8 block

    r[64] = &b[8];
    r[65] = &b[9];
    r[66] = &b[BLOCK_SIZE * 1 + 8];
    r[67] = &b[BLOCK_SIZE * 1 + 9];
    r[68] = &b[10];
    r[69] = &b[11];
    r[70] = &b[BLOCK_SIZE * 1 + 10];
    r[71] = &b[BLOCK_SIZE * 1 + 11];
    r[72] = &b[BLOCK_SIZE * 2 + 8];
    r[73] = &b[BLOCK_SIZE * 2 + 9];
    r[74] = &b[BLOCK_SIZE * 3 + 8];
    r[75] = &b[BLOCK_SIZE * 3 + 9];
    r[76] = &b[BLOCK_SIZE * 2 + 10];
    r[77] = &b[BLOCK_SIZE * 2 + 11];
    r[78] = &b[BLOCK_SIZE * 3 + 10];
    r[79] = &b[BLOCK_SIZE * 3 + 11];
    // Done with top-left --> right 2 4x4 block

    r[80] = &b[12];
    r[81] = &b[13];
    r[82] = &b[BLOCK_SIZE * 1 + 12];
    r[83] = &b[BLOCK_SIZE * 1 + 13];
    r[84] = &b[14];
    r[85] = &b[15];
    r[86] = &b[BLOCK_SIZE * 1 + 14];
    r[87] = &b[BLOCK_SIZE * 1 + 15];
    r[88] = &b[BLOCK_SIZE * 2 + 12];
    r[89] = &b[BLOCK_SIZE * 2 + 13];
    r[90] = &b[BLOCK_SIZE * 3 + 12];
    r[91] = &b[BLOCK_SIZE * 3 + 13];
    r[92] = &b[BLOCK_SIZE * 2 + 14];
    r[93] = &b[BLOCK_SIZE * 2 + 15];
    r[94] = &b[BLOCK_SIZE * 3 + 14];
    r[95] = &b[BLOCK_SIZE * 3 + 15];
    // Done with top-left --> right 3 4x4 block

    r[96] = &b[BLOCK_SIZE * 4 + 8];
    r[97] = &b[BLOCK_SIZE * 4 + 9];
    r[98] = &b[BLOCK_SIZE * 5 + 8];
    r[99] = &b[BLOCK_SIZE * 5 + 9];
    r[100] = &b[BLOCK_SIZE * 4 + 10];
    r[101] = &b[BLOCK_SIZE * 4 + 11];
    r[102] = &b[BLOCK_SIZE * 5 + 10];
    r[103] = &b[BLOCK_SIZE * 5 + 11];
    r[104] = &b[BLOCK_SIZE * 6 + 8];
    r[105] = &b[BLOCK_SIZE * 6 + 9];
    r[106] = &b[BLOCK_SIZE * 7 + 8];
    r[107] = &b[BLOCK_SIZE * 7 + 9];
    r[108] = &b[BLOCK_SIZE * 6 + 10];
    r[109] = &b[BLOCK_SIZE * 6 + 11];
    r[110] = &b[BLOCK_SIZE * 7 + 10];
    r[111] = &b[BLOCK_SIZE * 7 + 11];
    // Done with top-left --> down 1 --> right 2 4x4 block

    r[112] = &b[BLOCK_SIZE * 4 + 12];
    r[113] = &b[BLOCK_SIZE * 4 + 13];
    r[114] = &b[BLOCK_SIZE * 5 + 12];
    r[115] = &b[BLOCK_SIZE * 5 + 13];
    r[116] = &b[BLOCK_SIZE * 4 + 14];
    r[117] = &b[BLOCK_SIZE * 4 + 15];
    r[118] = &b[BLOCK_SIZE * 5 + 14];
    r[119] = &b[BLOCK_SIZE * 5 + 15];
    r[120] = &b[BLOCK_SIZE * 6 + 12];
    r[121] = &b[BLOCK_SIZE * 6 + 13];
    r[122] = &b[BLOCK_SIZE * 7 + 12];
    r[123] = &b[BLOCK_SIZE * 7 + 13];
    r[124] = &b[BLOCK_SIZE * 6 + 14];
    r[125] = &b[BLOCK_SIZE * 6 + 15];
    r[126] = &b[BLOCK_SIZE * 7 + 14];
    r[127] = &b[BLOCK_SIZE * 7 + 15];
    // Done with top-left --> down 1 --> right 3 4x4 block

    r[128] = &b[BLOCK_SIZE * 8 + 0];
    r[129] = &b[BLOCK_SIZE * 8 + 1];
    r[130] = &b[BLOCK_SIZE * 9 + 0];
    r[131] = &b[BLOCK_SIZE * 9 + 1];
    r[132] = &b[BLOCK_SIZE * 8 + 2];
    r[133] = &b[BLOCK_SIZE * 8 + 3];
    r[134] = &b[BLOCK_SIZE * 9 + 2];
    r[135] = &b[BLOCK_SIZE * 9 + 3];
    r[136] = &b[BLOCK_SIZE * 10 + 0];
    r[137] = &b[BLOCK_SIZE * 10 + 1];
    r[138] = &b[BLOCK_SIZE * 11 + 0];
    r[139] = &b[BLOCK_SIZE * 11 + 1];
    r[140] = &b[BLOCK_SIZE * 10 + 2];
    r[141] = &b[BLOCK_SIZE * 10 + 3];
    r[142] = &b[BLOCK_SIZE * 11 + 2];
    r[143] = &b[BLOCK_SIZE * 11 + 3];
    r[144] = &b[BLOCK_SIZE * 8 + 4];
    r[145] = &b[BLOCK_SIZE * 8 + 5];
    r[146] = &b[BLOCK_SIZE * 9 + 4];
    r[147] = &b[BLOCK_SIZE * 9 + 5];
    r[148] = &b[BLOCK_SIZE * 8 + 6];
    r[149] = &b[BLOCK_SIZE * 8 + 7];
    r[150] = &b[BLOCK_SIZE * 9 + 6];
    r[151] = &b[BLOCK_SIZE * 9 + 7];
    r[152] = &b[BLOCK_SIZE * 10 + 4];
    r[153] = &b[BLOCK_SIZE * 10 + 5];
    r[154] = &b[BLOCK_SIZE * 11 + 4];
    r[155] = &b[BLOCK_SIZE * 11 + 5];
    r[156] = &b[BLOCK_SIZE * 10 + 6];
    r[157] = &b[BLOCK_SIZE * 10 + 7];
    r[158] = &b[BLOCK_SIZE * 11 + 6];
    r[159] = &b[BLOCK_SIZE * 11 + 7];
    r[160] = &b[BLOCK_SIZE * 12 + 0];
    r[161] = &b[BLOCK_SIZE * 12 + 1];
    r[162] = &b[BLOCK_SIZE * 13 + 0];
    r[163] = &b[BLOCK_SIZE * 13 + 1];
    r[164] = &b[BLOCK_SIZE * 12 + 2];
    r[165] = &b[BLOCK_SIZE * 12 + 3];
    r[166] = &b[BLOCK_SIZE * 13 + 2];
    r[167] = &b[BLOCK_SIZE * 13 + 3];
    r[168] = &b[BLOCK_SIZE * 14 + 0];
    r[169] = &b[BLOCK_SIZE * 14 + 1];
    r[170] = &b[BLOCK_SIZE * 15 + 0];
    r[171] = &b[BLOCK_SIZE * 15 + 1];
    r[172] = &b[BLOCK_SIZE * 14 + 2];
    r[173] = &b[BLOCK_SIZE * 14 + 3];
    r[174] = &b[BLOCK_SIZE * 15 + 2];
    r[175] = &b[BLOCK_SIZE * 15 + 3];
    r[176] = &b[BLOCK_SIZE * 12 + 4];
    r[177] = &b[BLOCK_SIZE * 12 + 5];
    r[178] = &b[BLOCK_SIZE * 13 + 4];
    r[179] = &b[BLOCK_SIZE * 13 + 5];
    r[180] = &b[BLOCK_SIZE * 12 + 6];
    r[181] = &b[BLOCK_SIZE * 12 + 7];
    r[182] = &b[BLOCK_SIZE * 13 + 6];
    r[183] = &b[BLOCK_SIZE * 13 + 7];
    r[184] = &b[BLOCK_SIZE * 14 + 4];
    r[185] = &b[BLOCK_SIZE * 14 + 5];
    r[186] = &b[BLOCK_SIZE * 15 + 4];
    r[187] = &b[BLOCK_SIZE * 15 + 5];
    r[188] = &b[BLOCK_SIZE * 14 + 6];
    r[189] = &b[BLOCK_SIZE * 14 + 7];
    r[190] = &b[BLOCK_SIZE * 15 + 6];
    r[191] = &b[BLOCK_SIZE * 15 + 7];
    // Done with 3rd 8x8

    r[192] = &b[BLOCK_SIZE * 8 + 8];
    r[193] = &b[BLOCK_SIZE * 8 + 9];
    r[194] = &b[BLOCK_SIZE * 9 + 8];
    r[195] = &b[BLOCK_SIZE * 9 + 9];
    r[196] = &b[BLOCK_SIZE * 8 + 10];
    r[197] = &b[BLOCK_SIZE * 8 + 11];
    r[198] = &b[BLOCK_SIZE * 9 + 10];
    r[199] = &b[BLOCK_SIZE * 9 + 11];
    r[200] = &b[BLOCK_SIZE * 10 + 8];
    r[201] = &b[BLOCK_SIZE * 10 + 9];
    r[202] = &b[BLOCK_SIZE * 11 + 8];
    r[203] = &b[BLOCK_SIZE * 11 + 9];
    r[204] = &b[BLOCK_SIZE * 10 + 10];
    r[205] = &b[BLOCK_SIZE * 10 + 11];
    r[206] = &b[BLOCK_SIZE * 11 + 10];
    r[207] = &b[BLOCK_SIZE * 11 + 11];
    r[208] = &b[BLOCK_SIZE * 8 + 12];
    r[209] = &b[BLOCK_SIZE * 8 + 13];
    r[210] = &b[BLOCK_SIZE * 9 + 12];
    r[211] = &b[BLOCK_SIZE * 9 + 13];
    r[212] = &b[BLOCK_SIZE * 8 + 14];
    r[213] = &b[BLOCK_SIZE * 8 + 15];
    r[214] = &b[BLOCK_SIZE * 9 + 14];
    r[215] = &b[BLOCK_SIZE * 9 + 15];
    r[216] = &b[BLOCK_SIZE * 10 + 12];
    r[217] = &b[BLOCK_SIZE * 10 + 13];
    r[218] = &b[BLOCK_SIZE * 11 + 12];
    r[219] = &b[BLOCK_SIZE * 11 + 13];
    r[220] = &b[BLOCK_SIZE * 10 + 14];
    r[221] = &b[BLOCK_SIZE * 10 + 15];
    r[222] = &b[BLOCK_SIZE * 11 + 14];
    r[223] = &b[BLOCK_SIZE * 11 + 15];
    r[224] = &b[BLOCK_SIZE * 12 + 8];
    r[225] = &b[BLOCK_SIZE * 12 + 9];
    r[226] = &b[BLOCK_SIZE * 13 + 8];
    r[227] = &b[BLOCK_SIZE * 13 + 9];
    r[228] = &b[BLOCK_SIZE * 12 + 10];
    r[229] = &b[BLOCK_SIZE * 12 + 11];
    r[230] = &b[BLOCK_SIZE * 13 + 10];
    r[231] = &b[BLOCK_SIZE * 13 + 11];
    r[232] = &b[BLOCK_SIZE * 14 + 8];
    r[233] = &b[BLOCK_SIZE * 14 + 9];
    r[234] = &b[BLOCK_SIZE * 15 + 8];
    r[235] = &b[BLOCK_SIZE * 15 + 9];
    r[236] = &b[BLOCK_SIZE * 14 + 10];
    r[237] = &b[BLOCK_SIZE * 14 + 11];
    r[238] = &b[BLOCK_SIZE * 15 + 10];
    r[239] = &b[BLOCK_SIZE * 15 + 11];
    r[240] = &b[BLOCK_SIZE * 12 + 12];
    r[241] = &b[BLOCK_SIZE * 12 + 13];
    r[242] = &b[BLOCK_SIZE * 13 + 12];
    r[243] = &b[BLOCK_SIZE * 13 + 13];
    r[244] = &b[BLOCK_SIZE * 12 + 14];
    r[245] = &b[BLOCK_SIZE * 12 + 15];
    r[246] = &b[BLOCK_SIZE * 13 + 14];
    r[247] = &b[BLOCK_SIZE * 13 + 15];
    r[248] = &b[BLOCK_SIZE * 14 + 12];
    r[249] = &b[BLOCK_SIZE * 14 + 13];
    r[250] = &b[BLOCK_SIZE * 15 + 12];
    r[251] = &b[BLOCK_SIZE * 15 + 13];
    r[252] = &b[BLOCK_SIZE * 14 + 14];
    r[253] = &b[BLOCK_SIZE * 14 + 15];
    r[254] = &b[BLOCK_SIZE * 15 + 14];
    r[255] = &b[BLOCK_SIZE * 15 + 15];
    // Done with 4th 8x8

    r
}

fn block_tree_references_to_bitvec<'a>(r: &BlockTreeReferences<'a>) -> BitVec<u8, Msb0> {
    let mut bv = BitVec::new();
    let mut d_values = vec![];
    for i in 0..r.len() / 4 {
        let ref_d = r[i * 4].unwrap().d;
        d_values.push(ref_d);

        // First pixel in 2x2
        bv.push(true);

        match r[i * 4 + 1].unwrap().d == ref_d {
            true => {
                bv.push(true);
            }
            false => {
                bv.push(false);
                // d_values.push(r[i * 4 + 1].unwrap().d); // TODO refactor out
            }
        }
        match r[i * 4 + 2].unwrap().d == ref_d {
            true => {
                bv.push(true);
            }
            false => {
                bv.push(false);
                // d_values.push(r[i * 4 + 2].unwrap().d); // TODO refactor out
            }
        }
        match r[i * 4 + 3].unwrap().d == ref_d {
            true => {
                bv.push(true);
            }
            false => {
                bv.push(false);
                // d_values.push(r[i * 4 + 3].unwrap().d); // TODO refactor out
            }
        }
    }

    let (bv_2, d_values) = by_n_n(bv, 2, d_values);
    let (bv_4, d_values) = by_n_n(bv_2, 4, d_values);
    let (bv_8, d_values) = by_n_n(bv_4, 8, d_values);
    dbg!(bv_8.clone());
    dbg!(d_values);

    bv_8
}

fn by_n_n(
    bv_2_2: BitVec<u8, Msb0>,
    divisor: usize,
    mut d_values: Vec<D>,
) -> (BitVec<u8, Msb0>, Vec<D>) {
    let mut bv_n_n = bitvec![u8, Msb0;];
    let mut iter = bv_2_2.iter();

    let mut bv_end = bitvec![u8, Msb0;];

    let mut d_values_pos = 0;

    for i in 0..(BLOCK_SIZE / divisor) * (BLOCK_SIZE / divisor) {
        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        let c = iter.next().unwrap();
        let d = iter.next().unwrap();

        match *a && *b && *c && *d {
            true => {
                bv_n_n.push(true);
                // if divisor > 2 {
                //     assert_eq!(d_values[d_values_pos], d_values[d_values_pos + 1]);
                //     assert_eq!(d_values[d_values_pos], d_values[d_values_pos + 2]);
                //     assert_eq!(d_values[d_values_pos], d_values[d_values_pos + 3]);
                //     d_values.remove(d_values_pos + 1);
                //     d_values.remove(d_values_pos + 2);
                //     d_values.remove(d_values_pos + 3);
                //     d_values_pos += 1;
                // }
            }
            false => {
                bv_n_n.push(false);
                // d_values_pos += 4;

                // Push the difference information to the very end
                bv_end.push(*a);
                bv_end.push(*b);
                bv_end.push(*c);
                bv_end.push(*d);
            }
        }
    }

    bv_n_n.append(&mut bv_end);
    loop {
        let tmp = iter.next();
        match tmp {
            Some(val) => bv_n_n.push(*val),
            None => break,
        }
    }
    (bv_n_n, d_values)
}
use bitvec::slice::Iter;

fn encode_events<'a>(
    mut iter: &mut Iter<u8, Msb0>,
    r: &BlockTreeReferences<'a>,
    divisor: usize,
    mut output: &mut BitVec<u8, Msb0>,
    mut ref_offset: &mut usize,
) {
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    let c = iter.next().unwrap();
    let d = iter.next().unwrap();

    if divisor == 2 {
        for i in *ref_offset..*ref_offset + (divisor) * (divisor) {
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().d.to_be_bytes(),
            ));
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().delta_t.to_be_bytes(),
            ));
            // output_test.push(r[i].unwrap().delta_t);
        }
        *ref_offset += (divisor) * (divisor);
        return;
    }

    if *a {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &r[*ref_offset].unwrap().d.to_be_bytes(),
        ));
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().delta_t.to_be_bytes(),
            ));
            // output_test.push(r[i].unwrap().delta_t);
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        encode_events(iter, r, divisor / 2, &mut output, ref_offset);
    }

    if *b {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &r[*ref_offset].unwrap().d.to_be_bytes(),
        ));
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().delta_t.to_be_bytes(),
            ));
            // output_test.push(r[i].unwrap().delta_t);
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        encode_events(iter, r, divisor / 2, &mut output, ref_offset);
    }

    if *c {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &r[*ref_offset].unwrap().d.to_be_bytes(),
        ));
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().delta_t.to_be_bytes(),
            ));
            // output_test.push(r[i].unwrap().delta_t);
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        encode_events(iter, r, divisor / 2, &mut output, ref_offset);
    }

    if *d {
        output.append(&mut BitVec::<u8, Msb0>::from_slice(
            &r[*ref_offset].unwrap().d.to_be_bytes(),
        ));
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            output.append(&mut BitVec::<u8, Msb0>::from_slice(
                &r[i].unwrap().delta_t.to_be_bytes(),
            ));
            // output_test.push(r[i].unwrap().delta_t);
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        encode_events(iter, r, divisor / 2, &mut output, ref_offset);
    }
}

fn decode_events<'a>(
    mut iter_tree: &mut Iter<u8, Msb0>,
    mut events: &mut BitVec<u8, Msb0>,
    mut events_pos: &mut usize,
    mut r: &mut Block2,
    divisor: usize,
    mut ref_offset: &mut usize,
) {
    let a = iter_tree.next().unwrap();
    let b = iter_tree.next().unwrap();
    let c = iter_tree.next().unwrap();
    let d = iter_tree.next().unwrap();

    if divisor == 2 {
        for i in *ref_offset..*ref_offset + (divisor) * (divisor) {
            let mut d: D = events[*events_pos..*events_pos + 8].load_be();
            *events_pos += 8;
            let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
            *events_pos += 32;
            r[i] = Some(EventCoordless { d, delta_t });
        }
        *ref_offset += (divisor) * (divisor);
        return;
    }

    if *a {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
            *events_pos += 32;
            r[i] = Some(EventCoordless { d, delta_t });
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        decode_events(iter_tree, events, events_pos, r, divisor / 2, ref_offset);
    }

    if *b {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
            *events_pos += 32;
            r[i] = Some(EventCoordless { d, delta_t });
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        decode_events(iter_tree, events, events_pos, r, divisor / 2, ref_offset);
    }

    if *c {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;
        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
            *events_pos += 32;
            r[i] = Some(EventCoordless { d, delta_t });
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        decode_events(iter_tree, events, events_pos, r, divisor / 2, ref_offset);
    }

    if *d {
        let mut d: D = events[*events_pos..*events_pos + 8].load_be();
        *events_pos += 8;

        for i in *ref_offset..*ref_offset + (divisor / 2) * (divisor / 2) {
            let delta_t: DeltaT = events[*events_pos..*events_pos + 32].load_be();
            *events_pos += 32;
            r[i] = Some(EventCoordless { d, delta_t });
        }
        *ref_offset += (divisor / 2) * (divisor / 2);
    } else {
        decode_events(iter_tree, events, events_pos, r, divisor / 2, ref_offset);
    }
}

// #[derive(Default)]
// struct BasePixel<'a> {
//     d_val: D,
//     parent: Block2b2<'a>,
// }
//
// #[derive(Default)]
// struct Block2b2<'a> {
//     uniform: bool,
//     d_val: D,
//     parent: Block4b4<'a>,
// }
//
// #[derive(Default)]
// struct Block4b4<'a> {
//     uniform: bool,
//     d_val: D,
//     parent: &'a Block8b8<'a>,
// }
//
// #[derive(Default)]
// struct Block8b8<'a> {
//     uniform: bool,
//     d_val: D,
//     parent: &'a Block16b16<'a>,
// }
//
// #[derive(Default)]
// struct Block16b16<'a> {
//     uniform: bool,
//     d_val: Option<D>,
//     parent: &'a Block32b32<'a>,
// }
//
// #[derive(Default)]
// struct Block32b32<'a> {
//     uniform: bool,
//     d_val: D,
//     parent: &'a Block64b64,
// }
//
// #[derive(Default)]
// struct Block64b64 {
//     uniform: bool,
//     d_val: D,
// }

#[cfg(test)]
mod tests {
    use crate::codec::compressed::mod3::{
        block_tree_references, block_tree_references_to_bitvec, decode_events, encode_events,
    };
    use crate::codec::compressed::{
        by_2_2, decode_block, encode_block, raw_block_idx, Block, Cube, BLOCK_SIZE,
    };
    use crate::framer::driver::EventCoordless;
    use crate::{DeltaT, Event};
    use bitvec::prelude::*;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread::sleep;
    use std::time::Duration;

    // /// Create blocking tree of size `BLOCK_SIZE`
    // fn setup_64_block(block: &Block) -> Block64b64<'static> {
    //     let tree_block = Block64b64::default();
    //
    //     tree_block
    // }

    #[test]
    fn test_setup_64_block() {
        let mut block = [None; BLOCK_SIZE * BLOCK_SIZE];
        let mut dummy_event = EventCoordless::default();

        for (idx, event) in block.iter_mut().enumerate() {
            *event = Some(EventCoordless {
                d: 0,
                delta_t: idx as DeltaT,
            });
        }

        dummy_event.d = 7;
        block[raw_block_idx(0, 0)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(0, 1)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 0)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 1)].as_mut().unwrap().d = dummy_event.d;

        // Make the 1st and 3rd pixels match in 2nd block
        block[raw_block_idx(0, 2)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(1, 2)].as_mut().unwrap().d = dummy_event.d;

        dummy_event.d = 4;
        // Make the 1st and 4th pixels match in 4th block
        block[raw_block_idx(2, 2)].as_mut().unwrap().d = dummy_event.d;
        block[raw_block_idx(3, 3)].as_mut().unwrap().d = dummy_event.d;

        let tree_references = block_tree_references(&block);

        // Iterate the tree references and output a bool for each 2x2 sub-block d-values matching
        for i in 0..block.len() / 4 {
            let mut d_val = tree_references[i * 4].as_ref().unwrap().d;
            let mut uniform = true;
            for j in 1..4 {
                if tree_references[i * 4 + j].as_ref().unwrap().d != d_val {
                    uniform = false;
                    break;
                }
            }
            if i == 1 || i == 3 {
                assert!(!uniform);
            } else {
                assert!(uniform);
            }
        }

        let bv = block_tree_references_to_bitvec(&tree_references);

        // Problematic blocks are in the TL 8x8 section
        assert!(!bv[0]);
        assert!(bv[1]);
        assert!(bv[2]);
        assert!(bv[3]);

        // Problematic blocks are in the TL 4x4 section
        assert!(!bv[4]);
        assert!(bv[5]);
        assert!(bv[6]);
        assert!(bv[7]);

        // TL 2x2 is uniform with d_val = 7
        assert!(bv[8]);
        // TR 2x2 is not uniform
        assert!(!bv[9]);
        // BL 2x2 is uniform with d_val = 0
        assert!(bv[10]);
        // BR 2x2 is not uniform
        assert!(!bv[11]);

        // Dive into TR 2x2. 2nd and 4th pixels differ
        assert!(bv[12]);
        assert!(!bv[13]);
        assert!(bv[14]);
        assert!(!bv[15]);

        // Dive into BR 2x2. 2nd and 3rd pixels differ
        assert!(bv[16]);
        assert!(!bv[17]);
        assert!(!bv[18]);
        assert!(bv[19]);

        let mut iter = bv.iter();

        let mut output = BitVec::new();

        let mut ref_offset = 0;

        encode_events(
            &mut iter,
            &tree_references,
            BLOCK_SIZE,
            &mut output,
            &mut ref_offset,
        );

        let mut block_decoded = [None; BLOCK_SIZE * BLOCK_SIZE];
        let mut tree_references_decoded = block_tree_references(&block_decoded);

        let mut iter_tree = bv.iter();
        let mut events = output;
        let mut output_block: [Option<EventCoordless>; 256] = [None; BLOCK_SIZE * BLOCK_SIZE];
        let mut events_pos = 0;
        let mut ref_offset = 0;
        decode_events(
            &mut iter_tree,
            &mut events,
            &mut events_pos,
            &mut block_decoded,
            BLOCK_SIZE,
            &mut ref_offset,
        );

        for i in 0..BLOCK_SIZE * BLOCK_SIZE {
            assert_eq!(block_decoded[i], *tree_references[i]);
        }

        // assert_eq!(output_block, block);
    }
}
