
use minifb::{MouseMode, Window, WindowOptions, ScaleMode, Scale};
use anyhow::{anyhow, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Seek, SeekFrom};
use std::{collections::HashMap, fs::File, hash::Hash, io::Read};

use raqote::*;

const WIDTH: usize = 800;
const HEIGHT: usize = 600;

fn bit_is_set(flag: u8, flag_bit_index: u8) -> bool {
    // 00100000, 6 -> 00000001 & 00000001
    return ((flag >> flag_bit_index) & 1) == 1;
}

fn get_coordinates(cursor: &mut Cursor<Vec<u8>>, flags: &Vec<u8>, is_x: bool) -> Result<Vec<i16>> {
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
#[derive(Debug)]
struct GlyphData {
    x_coords: Vec<i16>,
    y_coords: Vec<i16>,
    contour_end_indices: Vec<u16>,
    is_simple: bool
}

impl GlyphData {
    fn from_cursor(cursor: &mut Cursor<Vec<u8>>) -> Result<GlyphData> {
        //let num_contour_end_indices
        let mut contour_end_indices: Vec<u16> = Vec::new();

        let num_contour_end_indices = cursor.read_i16::<BigEndian>()?; //slice_as_u16_be!(data, &start_bytes);
        cursor.seek(SeekFrom::Current(8))?; // skip bounding box for the character data
        if num_contour_end_indices >= 0 {
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

            let mut x_coords: Vec<i16> = get_coordinates(cursor, &flags, true)?;
            let mut y_coords: Vec<i16> = get_coordinates(cursor, &flags, false)?;

            Ok(GlyphData {
                x_coords,
                y_coords,
                contour_end_indices,
                is_simple:true
            })
        } else {
            println!("Skipping compound glyph");
            Ok(GlyphData {
                x_coords: vec![],
                y_coords: vec![],
                contour_end_indices: vec![],
                is_simple:false
            })
        }
    }
}
/*
#[derive(Debug)]
struct FontHeader {
    version: u32,
    font_revision: u32,

}*/

#[derive(Debug)]
struct Font {
    tables: HashMap<String, (u32, u32, u32)>, // tag :(checkSum, offset, length)
    glyph_data: Vec<GlyphData>,
    cursor: Cursor<Vec<u8>>

}

impl Font {
    pub fn read_truetype(filename: &str) -> Result<Font> {
        if let Ok(mut font_file) = File::open(filename) {
            let mut contents = Vec::<u8>::new();
            font_file.read_to_end(&mut contents);
            let file_len:usize = contents.len();
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

            let (_, head_table_offset, _) = tables.get("head").unwrap();
            cursor.seek(SeekFrom::Start((head_table_offset + 50) as u64)); // skip some 50 bytes of additional information

            let use_two_byte_entry = cursor.read_i16::<BigEndian>()? == 0; // check if we use two bye entries (indexToLocFormat)

            let (_, location_table_offset, _) = tables.get("loca").unwrap();

            // working with the glyph table
            let (_, glyph_table_offset, glyph_table_len) = tables.get("glyf").unwrap();
            // cursor.seek(SeekFrom::Start(*glyph_table_offset as u64));

            let mut glyph_locations: Vec<u64> = vec![0u64; num_glyphs as usize];
            let mut glyph_data_list = Vec::<GlyphData>::new();

            for i in 0..(num_glyphs as u64) {
                cursor.seek(SeekFrom::Start(
                    (*location_table_offset as u64 + i * (if use_two_byte_entry { 2 } else { 4 }))
                        as u64,
                ))?;

                let glyph_start_offset = if use_two_byte_entry {
                    cursor.read_u16::<BigEndian>()? as u32 * 2u32
                } else {
                    cursor.read_u32::<BigEndian>()?
                };

                let glyph_offset = *glyph_table_offset + glyph_start_offset;
                if glyph_offset as usize > file_len {
                    return Err(anyhow!("Glyph offset beyond file size: offset = {}, file size = {}", glyph_offset, file_len));
                }

                glyph_locations[i as usize] = glyph_offset as u64;

            }


            for i in 0..(num_glyphs as u64) {

                cursor.seek(SeekFrom::Start(
                    (*glyph_locations.get(i as usize).unwrap()),
                ));
                match GlyphData::from_cursor(&mut cursor) {
                    Ok(glyph_data) => {
                        println!("{i} \n");
                        glyph_data_list.push(glyph_data);
                    }
                    Err(err) => println!("Error : {err}"),
                }
            }




            println!("Number of tables : {num_tables}");
            return Ok(Font {
                tables,
                glyph_data:glyph_data_list,
                cursor
            });
        } else {
            println!("Failed to read file contents");
            Err(anyhow!("Failed to read file contents"))
        }
    }
}

fn main() {
    //let font_read = Font::read_truetype("Inconsolata-Regular.ttf"); //SourceCodePro-Regular.ttf
    //let font_read = Font::read_truetype("SourceCodePro-Regular.ttf"); //s

    let mut window = Window::new("Text renderer", WIDTH, HEIGHT, WindowOptions {
        ..WindowOptions::default()
    }).unwrap();


    let size = window.get_size();
    let mut dt = DrawTarget::new(size.0 as i32, size.1 as i32);
    loop {
        dt.clear(SolidSource::from_unpremultiplied_argb(0xff, 0xff, 0xff, 0xff));
        let mut pb = PathBuilder::new();
        if let Some(pos) = window.get_mouse_pos(MouseMode::Clamp) {

            pb.rect(pos.0, pos.1, 100., 130.);
            let path = pb.finish();
            dt.fill(&path, &Source::Solid(SolidSource::from_unpremultiplied_argb(0xff, 0, 0xff, 0)), &DrawOptions::new());


            window.update_with_buffer(dt.get_data(), size.0, size.1).unwrap();
        }
    }
}
