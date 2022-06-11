use std::env;
use std::fs;
use std::mem;
use std::str;

fn main() {
    use std::process::exit;
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("usage: {} <image>", args[0]);
        exit(1);
    }

    let file = fs::read(&args[1]).unwrap();
    let image = read_png(file).unwrap();

    println!("{:?}", image);
}

#[derive(Debug)]
struct PNG {
    width: u32,
    height: u32,
}

#[derive(Debug)]
enum PNGChunk<'a> {
    IHDR {
        width: u32,
        height: u32,
        bit_depth: u8,
        color_type: u8,
        compression_method: u8,
        filter_method: u8,
        interlace_method: u8,
    },
    PLTE,
    IDAT(&'a [u8]),
    IEND,
    Ancillary,
}

fn read_png(file: Vec<u8>) -> Option<PNG> {
    let png_signature = [137, 80, 78, 71, 13, 10, 26, 10];
    for i in 0..8 {
        if file[i] != png_signature[i] {
            return None;
        }
    }
    let chunks = read_chunks(&file[8..])?;

    // extract the width and height from the header chunk, also check if we support the image
    // format that we recieved
    let image_width;
    let image_height;
    match chunks[0] {
        PNGChunk::IHDR {
            width,
            height,
            bit_depth,
            color_type,
            compression_method,
            filter_method,
            interlace_method,
        } => {
            image_width = width;
            image_height = height;

            // we'll only support 8 bit samples
            if bit_depth != 8 {
                return None;
            }

            // TODO: for now we'll only support truecolor images, in the future we'll want to 
            // support truecolor with alpha (aka color type 6)
            if color_type != 2 {
                return None
            }

            // only DEFLATE (RFC-1950, RFC-1951) compression is defined by the PNG standard
            if compression_method != 0 {
                return None;
            }

            // only adaptive filtering is defined by the standard
            if filter_method != 0 {
                return None;
            }

            // TODO: for now we'll only support images with no interlacing, we need to find out if
            // there's any use in supporting Adam7 interlacing
            if interlace_method != 0 {
                return None
            }
        },

        // if the first chunk isn't the header chunk thats an error according to the spec
        _ => return None

    }

    // get the image data and see if there are errors in the rest of the chunks
    let mut image_data = Vec::<u8>::new();
    for chunk in &chunks[1..] {
        match chunk {
            // more than one ihdr chunk is an error
            PNGChunk::IHDR {..} => return None,
            PNGChunk::IDAT(chunk_data) => image_data.extend_from_slice(chunk_data),
            // we don't support palletized images
            PNGChunk::PLTE => return None,
            _ => ()
        }
    }
    decompress_image(&image_data)?;

    Some(PNG { width: image_width, height: image_height })
}

fn read_chunks(mut file: &[u8]) -> Option<Vec<PNGChunk>> {
    let mut chunks = Vec::new();

    while file.len() != 0 {
        // all chunks start with the length of their data field and their type as a FourCC code
        let length = read_u32(&mut file)?;
        let chunk_type = read_bytes(&mut file, 4)?;
        let chunk_type_str = match str::from_utf8(chunk_type) {
            Ok(s) => s,
            Err(_) => return None,
        };

        let mut chunk_data = read_bytes(&mut file, length as usize)?;
        chunks.push(match chunk_type_str {
            "IHDR" => {
                let width = read_u32(&mut chunk_data)?;
                let height = read_u32(&mut chunk_data)?;
                let bit_depth = read_u8(&mut chunk_data)?;
                let color_type = read_u8(&mut chunk_data)?;
                let compression_method = read_u8(&mut chunk_data)?;
                let filter_method = read_u8(&mut chunk_data)?;
                let interlace_method = read_u8(&mut chunk_data)?;
                PNGChunk::IHDR {
                    width,
                    height,
                    bit_depth,
                    color_type,
                    compression_method,
                    filter_method,
                    interlace_method,
                }
            }
            "IDAT" => PNGChunk::IDAT(chunk_data),
            "PLTE" => PNGChunk::PLTE,
            "IEND" => PNGChunk::IEND,
            _ => match chunk_type[0] & (1 << 5) == (1 << 5) {
                true => PNGChunk::Ancillary,
                false => return None,
            },
        });

        // all chunks end with a 4 byte CRC at the end, we aren't doing error checking so there's
        // nothing of interest to do with this value
        read_u32(&mut file);
    }

    Some(chunks)
}

fn decompress_image(mut image_data: &[u8]) ->Option<()> {
    let cmf = read_u8(&mut image_data)?;
    let flg = read_u8(&mut image_data)?;

    // compression_method
    let cm = cmf & 0xf;

    // if a preset dictionary is set
    let fdict = (flg >> 5) & 0x1;

    // compression level
    let flevel = flg >> 6;

    // according to the standard if any other value is set for these variables it's an error
    if cm != 8 || fdict != 0 {
        return None;
    }

    let mut final_block = false;
    while !final_block {
        let header = read_u8(&mut image_data)?;
        let bfinal = header & 0x1;
        let btype = (header >> 1) & 0x3;

        println!("BTYPE: {} BFINAL: {}", btype, bfinal);

        if bfinal == 1 {
            final_block = true;
        }

        match btype {
            0b00 => {
                let len = read_u16(&mut image_data)?;                
                let _nlen = read_u16(&mut image_data)?;
                println!("Uncompressed Chunk Length: {}", len);
                read_bytes(&mut image_data, len as usize);
            },
            0b01 => {
                break;
            },
            0b10 => break,
            _ => return None
        }
    }

    Some(())
}

/// get a u32 from the file data and advance the pointer. Returns None if there isn't enough space
/// left.
fn read_u32(file: &mut &[u8]) -> Option<u32> {
    let bytes = read_bytes(file, mem::size_of::<u32>())?;
    let num = u32::from_be_bytes(match bytes.try_into() {
        Ok(slice) => slice,
        Err(_) => return None,
    });

    Some(num)
}

/// get a u32 from the file data and advance the pointer. Returns None if there isn't enough space
/// left.
fn read_u16(file: &mut &[u8]) -> Option<u16> {
    let bytes = read_bytes(file, mem::size_of::<u16>())?;
    let num = u16::from_be_bytes(match bytes.try_into() {
        Ok(slice) => slice,
        Err(_) => return None,
    });

    Some(num)
}

/// get a u8 from the file data and advance the pointer. Returns None if there isn't enough space
/// left.
fn read_u8(file: &mut &[u8]) -> Option<u8> {
    let num_slice = read_bytes(file, 1)?;
    Some(num_slice[0])
}

static mut BITS_READ: usize = 0;

/// get an arbitrary amount of bytes from the file data and advance the pointer. Returns None if
/// there isn't enough space left
fn read_bytes<'a>(file: &mut &'a [u8], bytes: usize) -> Option<&'a [u8]> {
    unsafe {BITS_READ = 0};
    if file.len() < bytes {
        return None;
    }

    let (ret, rest) = file.split_at(bytes);
    *file = rest;

    Some(ret)
}

fn read_bits(file: &mut &[u8], bits: u32) -> Option<u32> {
    if bits > 32 {
        return None;
    }

    Some(0)
}