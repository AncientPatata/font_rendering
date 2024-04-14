use anyhow::{anyhow, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Seek, SeekFrom};
use std::{collections::HashMap, fs::File, hash::Hash, io::Read};

fn bit_is_set(flag: u8, flag_bit_index: u8) -> bool {
    // 00100000, 6 -> 00000001 & 00000001
    return ((flag >> flag_bit_index) & 1) == 1;
}

fn get_coordinates(cursor: &mut Cursor<Vec<u8>>, flags: Vec<u8>, is_x: bool) -> Result<Vec<i16>> {
    let num_points = flags.len();
    let mut coords: Vec<i16> = vec![0i16; num_points as usize];

    for i in 0..(num_points as usize) {
        coords[i] = if i == 0 { 0 } else { coords[i - 1] };
        let flag: u8 = flags[i as usize];
        let on_curve = bit_is_set(flag, 0);
        let mut is_short = false;
        let mut is_positive_short = false;
        if is_x {
            is_short = bit_is_set(flag, 1);
            is_positive_short = bit_is_set(flag, 4);
        } else {
            is_short = bit_is_set(flag, 2);
            is_positive_short = bit_is_set(flag, 5);
        }

        // coordinate offset is 1 byte
        if is_short {
            let offset: u8 = cursor.read_u8()?;
            let sign: i16 = if is_positive_short { 1 } else { -1 };
            coords[i] += sign * (offset as i16);
        } else if !is_positive_short {
            // coordinate offset value is represented by 2 byes (signed)
            coords[i] += cursor.read_i16::<BigEndian>()?;
        }
    }
    return Ok(coords);
}

struct GlyphData {
    x_coords: Vec<i16>,
    y_coords: Vec<i16>,
    contour_end_indices: Vec<u16>,
}

impl GlyphData {
    fn from_cursor(cursor: &mut Cursor<Vec<u8>>) -> Result<GlyphData> {
        //let num_contour_end_indices
        let mut contour_end_indices: Vec<u16> = Vec::new();

        let num_contour_end_indices = cursor.read_i16::<BigEndian>()?; //slice_as_u16_be!(data, &start_bytes);
        cursor.seek(SeekFrom::Current(8)); // skip bounding box for the character data
        for i in 0..num_contour_end_indices {
            contour_end_indices.push(cursor.read_u16::<BigEndian>()?)
        }

        let num_points = contour_end_indices.last().unwrap() + 1; // I'm guessing the last element in the contour indices represents the last point, and we just add one because points are indexed from 0

        // get number of instructions and skip them (instruction : 1 byte)
        let num_instructions = cursor.read_i16::<BigEndian>()?;
        cursor.seek(SeekFrom::Current(num_instructions as i64));

        // adding all of the flags
        let mut flags: Vec<u8> = Vec::new();

        let mut i = 0;
        while i < num_points {
            let flag: u8 = cursor.read_u8()?;
            flags.push(flag);

            // handle repeat
            if bit_is_set(flag, 3) {
                let num_repetitions: u8 = cursor.read_u8()?;
                for j in 0..num_repetitions {
                    flags.push(flag);
                }
                i += num_repetitions as u16;
            }
            i += 1;
        }

        let mut x_coords: Vec<i16> = vec![0i16; num_points as usize];
        let mut y_coords: Vec<i16> = vec![0i16; num_points as usize];

        // reading x coordinates
        for i in 0..(num_points as usize) {
            x_coords[i] = if i == 0 { 0 } else { x_coords[i - 1] };
            let flag: u8 = flags[i as usize];
            let on_curve = bit_is_set(flag, 0);

            let is_x_short = bit_is_set(flag, 1);
            let is_x_positive_short = bit_is_set(flag, 4);

            // coordinate offset is 1 byte
            if is_x_short {
                let offset: u8 = cursor.read_u8()?;
                let sign: i16 = if is_x_positive_short { 1 } else { -1 };
                x_coords[i] += sign * (offset as i16);
            } else if !is_x_positive_short {
                // coordinate offset value is represented by 2 byes (signed)
                x_coords[i] += cursor.read_i16::<BigEndian>()?;
            }
        }

        // reading y coordinates
        for i in 0..(num_points as usize) {
            y_coords[i] = if i == 0 { 0 } else { y_coords[i - 1] };
            let flag: u8 = flags[i as usize];
            let on_curve = bit_is_set(flag, 0);

            let is_y_short = bit_is_set(flag, 2);
            let is_y_positive_short = bit_is_set(flag, 5);

            // coordinate offset is 1 byte
            if is_y_short {
                let offset: u8 = cursor.read_u8()?;
                let sign: i16 = if is_y_positive_short { 1 } else { -1 };
                y_coords[i] += sign * (offset as i16);
            } else if !is_y_positive_short {
                // coordinate offset value is represented by 2 byes (signed)
                y_coords[i] += cursor.read_i16::<BigEndian>()?;
            }
        }

        Ok(GlyphData {
            x_coords,
            y_coords,
            contour_end_indices,
        })
    }
}

#[derive(Debug)]
struct Font {}

impl Font {
    pub fn read_truetype(filename: &str) -> Result<Font> {
        if let Ok(mut font_file) = File::open(filename) {
            let mut contents = Vec::<u8>::new();
            font_file.read_to_end(&mut contents);
            let mut cursor = Cursor::new(contents);
            cursor.seek(SeekFrom::Current(4)); // Skip scaler type
            let num_tables = cursor.read_u16::<BigEndian>()?;
            println!("Font file has {num_tables} tables");
            cursor.seek(SeekFrom::Current(2 + 2 + 2)); // Skip some of the fields in the file header

            let mut tables: HashMap<String, (u32, u32, u32)> = HashMap::new();
            for i in 0..num_tables {
                // tag : 4 | checkSum : 4 | offset : 4 | length : 4
                let mut buf = vec![0u8; 4];
                cursor.read_exact(&mut buf)?;
                let tag: String = String::from_utf8(buf).unwrap();
                let check_sum = cursor.read_u32::<BigEndian>()?;
                let offset = cursor.read_u32::<BigEndian>()?;
                let length = cursor.read_u32::<BigEndian>()?;
                println!(
                    "Table directory with tag {tag} --- offset = {offset} | length = {length}"
                );
                tables.insert(tag, (check_sum, offset, length));
            }

            // get number of glyphs
            let (_, maxp_table_offset, _) = tables.get("maxp").unwrap(); // TODO: Error handling on all of the unwraps
            cursor.seek(SeekFrom::Start(*maxp_table_offset as u64 + 4)); // we skip 4 bytes here for the "version number"
            let num_glyphs = cursor.read_u16::<BigEndian>()?;
            println!("Font contains {num_glyphs} glyphs");

            // working with the glyph table
            let (_, glyph_table_offset, glyph_table_len) = tables.get("glyf").unwrap();
            cursor.seek(SeekFrom::Start(*glyph_table_offset as u64));

            let mut glyph_data_list = Vec::<GlyphData>::new();

            for i in 0..num_glyphs {
                match GlyphData::from_cursor(&mut cursor) {
                    Ok(glyph_data) => {
                        println!("{i} \n");
                        glyph_data_list.push(glyph_data);
                    }
                    Err(err) => println!("Error : {err}"),
                }
            }

            if let Some(glyph) = glyph_data_list.get(0) {
                for i in 0..glyph.x_coords.len() {
                    let x = glyph.x_coords[i];
                    let y = glyph.y_coords[i];
                    println!("Point({x}, {y})");
                }
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
    let font_read = Font::read_truetype("Inconsolata-Regular.ttf");
}
