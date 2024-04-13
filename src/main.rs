use anyhow::{anyhow, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;
use std::{collections::HashMap, fs::File, hash::Hash, io::Read};

fn as_u32_be(array: &[u8; 4]) -> u32 {
    ((array[0] as u32) << 24)
        + ((array[1] as u32) << 16)
        + ((array[2] as u32) << 8)
        + ((array[3] as u32) << 0)
}

fn as_u16_be(array: &[u8; 2]) -> u16 {
    ((array[0] as u16) << 8) + ((array[1] as u16) << 0)
}

macro_rules! slice_as_u16_be {
    ($array:expr, $start_index:expr) => {
        as_u16_be(
            $array[($start_index)..($start_index + 2)]
                .try_into()
                .unwrap(),
        )
    };
}

macro_rules! slice_as_u32_be {
    ($array:expr, $start_index:expr) => {
        as_u32_be(
            $array[($start_index)..($start_index + 4)]
                .try_into()
                .unwrap(),
        )
    };
}

fn bit_is_set(flag: u8, flag_bit_index: u8) -> bool {
    // 00100000, 6 -> 00000001 & 00000001
    return ((flag >> flag_bit_index) & 1) == 1;
}

struct GlyphData {
    x_coords: Vec<i16>,
    y_coords: Vec<i16>,
    contour_end_indices: Vec<u16>,
}

impl GlyphData {
    fn from_file_contents(data: &Vec<u8>, start_bytes: &usize, glyph_end: &mut u32) -> GlyphData {
        //let num_contour_end_indices
        let mut x_coords: Vec<i16> = Vec::new();
        let mut y_coords: Vec<i16> = Vec::new();
        let mut contour_end_indices: Vec<u16> = Vec::new();

        let num_contour_end_indices = slice_as_u16_be!(data, &start_bytes);
        for i in 0..num_contour_end_indices {
            contour_end_indices.push(slice_as_u16_be!(data, start_bytes + 2 + 2 * (i as usize)))
        }

        let num_points = contour_end_indices.last().unwrap() + 1; // I'm guessing the last element in the contour indices represents the last point, and we just add one because points are indexed from 0

        let mut current_byte = start_bytes + 2 + 2 * (num_contour_end_indices as usize - 1) + 2;

        // get number of instructions and skip them
        let num_instructions = slice_as_u16_be!(data, current_byte);
        current_byte += 2;
        current_byte += num_instructions as usize; // each instruction is 1 byte;

        // adding all of the flags
        let mut flags: Vec<u8> = Vec::new();

        let mut i = 0;
        while i < num_points {
            let flag: u8 = data[current_byte];
            current_byte += 1;
            flags.push(flag);

            // handle repeat
            if bit_is_set(flag, 3) {
                let num_repetitions: u8 = data[current_byte];
                current_byte += 1; // TODO: probably better to group the additions ? or no. I think it's easier to mentally parse if written this way
                for j in 0..num_repetitions {
                    flags.push(data[current_byte]);
                    current_byte += 1;
                }
                i += num_repetitions as u16;
            }
            i += 1;
        }

        // reading x coordinates
        for i in 0..(num_points as usize) {
            x_coords[i] = if i == 0 { 0 } else { x_coords[i - 1] };
            let flag: u8 = flags[i as usize];
            let on_curve = bit_is_set(flag, 0);

            let is_x_short = bit_is_set(flag, 1);
            let is_x_positive_short = bit_is_set(flag, 4);

            // coordinate offset is 1 byte
            if is_x_short {
                let offset: u8 = data[current_byte];
                current_byte += 1;
                let sign: i16 = if is_x_positive_short { 1 } else { -1 };
                x_coords[i] += sign * (offset as i16);
            } else if !is_x_positive_short {
                // coordinate offset value is represented by 2 byes (signed)
                x_coords[i] += slice_as_u16_be!(data, current_byte) as i16;
                current_byte += 2;
            }
        }

        // reading y coordinates
        for i in 0..(num_points as usize) {
            y_coords[i] = if i == 0 { 0 } else { x_coords[i - 1] };
            let flag: u8 = flags[i as usize];
            let on_curve = bit_is_set(flag, 0);

            let is_y_short = bit_is_set(flag, 2);
            let is_y_positive_short = bit_is_set(flag, 5);

            // coordinate offset is 1 byte
            if is_y_short {
                let offset: u8 = data[current_byte];
                current_byte += 1;
                let sign: i16 = if is_y_positive_short { 1 } else { -1 };
                y_coords[i] += sign * (offset as i16);
            } else if !is_y_positive_short {
                // coordinate offset value is represented by 2 byes (signed)
                y_coords[i] += slice_as_u16_be!(data, current_byte) as i16;
                current_byte += 2;
            }
        }

        glyph_end = &mut (current_byte as u32);

        GlyphData {
            x_coords,
            y_coords,
            contour_end_indices,
        }
    }
}

struct Font {}

impl Font {
    pub fn read_truetype(filename: &str) -> Result<Font> {
        if let Ok(mut font_file) = File::open(filename) {
            let mut contents = Vec::<u8>::new();
            font_file.read_to_end(&mut contents);
            let cursor = Cursor::new(contents);
            let num_tables = as_u16_be(contents[4..6].try_into().unwrap());
            println!("Font file has {num_tables} tables");
            let table_dir_start = 4 + 2 + 2 + 2 + 2;
            let table_desc_len = 4 + 4 + 4 + 4;
            let table_dir_end = table_dir_start + (num_tables * table_desc_len) - 1;
            let mut tables: HashMap<&str, (u32, u32, u32)> = HashMap::new();
            for byte in (table_dir_start..=table_dir_end).step_by(table_desc_len as usize) {
                // tag : 4 | checkSum : 4 | offset : 4 | length : 4
                let tag =
                    std::str::from_utf8(&contents[(byte as usize)..((byte + 4) as usize)]).unwrap();
                let check_sum = as_u32_be(
                    &contents[((byte + 4) as usize)..((byte + 4 * 2) as usize)]
                        .try_into()
                        .unwrap(),
                );
                let offset = as_u32_be(
                    &contents[((byte + 4 * 2) as usize)..((byte + 4 * 3) as usize)]
                        .try_into()
                        .unwrap(),
                );
                let length = as_u32_be(
                    &contents[((byte + 4 * 3) as usize)..((byte + 4 * 4) as usize)]
                        .try_into()
                        .unwrap(),
                );
                tables.insert(tag, (check_sum, offset, length));
                // println!(
                //     "Table directory with tag {tag} --- offset = {offset} | length = {length}"
                // );
            }

            // get number of glyphs
            let (_, maxp_table_offset, _) = tables.get("maxp").unwrap(); // TODO: Error handling on all of the unwraps

            let num_glyphs = slice_as_u16_be!(contents, (maxp_table_offset + 4) as usize);
            println!("Font contains {num_glyphs} glyphs");

            // working with the glyph table
            let (_, glyph_table_offset, glyph_table_len) = tables.get("glyf").unwrap();
            let glyf_offset = *glyph_table_offset as usize;
            // reading the glyph table descriptor :

            let num_contours = slice_as_u16_be!(contents, glyf_offset);

            let x_min = slice_as_u16_be!(contents, glyf_offset + 2);

            let y_min = slice_as_u16_be!(contents, glyf_offset + 4);

            let x_max = slice_as_u16_be!(contents, glyf_offset + 6);

            let y_max = slice_as_u16_be!(contents, glyf_offset + 8);

            // going through all the glyphs and extracting the glyph data

            let mut glyph_data_list = Vec::<GlyphData>::new();

            let mut start_bytes = glyf_offset + 8;
            for i in 0..num_glyphs {
                let mut glyph_end = 0;
                glyph_data_list.push(GlyphData::from_file_contents(
                    &contents,
                    &start_bytes,
                    &mut glyph_end,
                ));
                start_bytes = glyph_end as usize;
            }

            println!("Number of tables : {num_tables}");
            return Ok(Font {});
        } else {
            println!("Failed to read file contents");
            Err(anyhow!("Failed to read file contents"))
        }
    }
}

fn main() {
    let font = Font::read_truetype("Inconsolata-Regular.ttf");
}
