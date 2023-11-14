use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[allow(dead_code)] // Suppress unused fields warning
#[derive(Debug, Deserialize, Clone)]
struct Event {
    t: u32,
    x: u16,
    y: u16,
    p: u8,
}

fn parse_header(file: &mut BufReader<File>) -> io::Result<(u64, u8, u8, Option<(u32, u32)>)> {
    file.seek(SeekFrom::Start(0))?; // Seek to the beginning of the file
    let mut bod = 0;
    let mut end_of_header = false;
    let mut num_comment_line = 0;
    let mut size = [None, None];

    // Parse header
    while !end_of_header {
        bod = file.seek(SeekFrom::Current(0))?; // Get the current position
        let mut line = Vec::new(); // Change to Vec<u8>
        file.read_until(b'\n', &mut line)?; // Read until newline as binary data
        if line.is_empty() || line[0] != b'%' {
            end_of_header = true;
        } else {
            let words: Vec<&[u8]> = line.split(|&x| x == b' ' || x == b'\t').collect(); // Use &[u8] instead of &str

            if words.len() > 1 {
                match words[1] {
                    b"Height" => {
                        size[0] = words.get(2).map(|s| {
                            std::str::from_utf8(s)
                                .ok()
                                .and_then(|s| s.parse().ok())
                        }).flatten();
                    }
                    b"Width" => {
                        size[1] = words.get(2).map(|s| {
                            std::str::from_utf8(s)
                                .ok()
                                .and_then(|s| s.parse().ok())
                        }).flatten();
                    }
                    _ => {}
                }
            }
            num_comment_line += 1;
        }
    }

    // Parse data
    file.seek(SeekFrom::Start(bod))?; // Seek back to the position after the header
    let (ev_type, ev_size) = if num_comment_line > 0 {
        // Read event type and size
        let mut buf = [0; 2]; // Adjust the buffer size based on your data size
        file.read_exact(&mut buf)?;
        let ev_type = buf[0];
        let ev_size = buf[1];

        (ev_type, ev_size)
    } else {
        (0, 0) // Placeholder values, replace with actual logic
    };
    bod = file.seek(SeekFrom::Current(0))?;
    Ok((bod, ev_type, ev_size, Some((size[0].unwrap_or(0), size[1].unwrap_or(0)))))
}

fn stream_td_data(
    reader: &mut BufReader<File>,
    offset: u64,
    count: usize,
    event_buffer: &mut Vec<Event>,
) -> io::Result<()> {
    for i in 0..count {
        // Move the file pointer to the specified offset for each iteration
        let current_offset = offset + (i as u64) * 8; // Assuming each record is 8 bytes
        reader.seek(SeekFrom::Start(current_offset))?;

        // Read one record
        let mut buffer = [0; 8]; // Adjust this size to match your record size
        reader.read_exact(&mut buffer)?;

        // Interpret the bytes as 't' and 'data'
        let t = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let data = i32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

        // Perform bitwise operations
        let x = (data & 16383) as u16;
        let y = ((data & 268419072) >> 14) as u16;
        let p = ((data & 268435456) >> 28) as u8;

        event_buffer.push(Event { t, x, y, p });
    }

    Ok(())
}


fn count_events(filename: &str) -> io::Result<usize> {
    let path = Path::new(filename);
    let mut file = BufReader::new(File::open(&path)?);

    let (bod, _, ev_size, _) = parse_header(&mut file)?;
    file.seek(SeekFrom::End(0))?;
    let eod = file.stream_position()? as u64;

    println!("eod: {}, bod: {}, ev_size: {}", eod, bod, ev_size); // Print debug information

    if (eod - bod) % u64::from(ev_size) != 0 {
        eprintln!("Warning: Unexpected format. eod: {}, bod: {}, ev_size: {}", eod, bod, ev_size);
    }

    // Check for unexpected end-of-file
    if eod < bod {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Unexpected end of file"));
    }
    Ok(((eod - bod) / u64::from(ev_size)) as usize)
}


fn main() {
    let file_path: &str = "/home/argha/Documents/github/prophesee_data/obj_010369_td.dat";
    let mut file = BufReader::new(File::open(file_path).unwrap());

    // Parse header
    let (bod, _, _, size) = parse_header(&mut file).unwrap();
    // Get the count of events from the file
    if let Ok(ev_count) = count_events(file_path) {
        // Prepare buffer for events
        let mut event_buffer: Vec<Event> = Vec::new();
        if let Err(err) = file.seek(SeekFrom::Start(bod)) {
            eprintln!("Error seeking file: {}", err);
            return;
        }

        // Stream data into the buffer
        if let Err(e) = stream_td_data(&mut file, 93, ev_count, &mut event_buffer) {
            eprintln!("Error: {}", e);
        }
    
        // for (i, event) in event_buffer.iter().enumerate() {
        //     println!(
        //         "Record {}: t: {}, x: {}, y: {}, p: {}",
        //         i + 1,
        //         event.t,
        //         event.x,
        //         event.y,
        //         event.p
        //     );
        // }
        // Display results
        if let Some((height, width)) = size {
            println!("Height: {}, Width: {}", height, width);
        }
        println!("Event Count: {}", event_buffer.len());
        println!("Loaded events: {:?}", event_buffer);
    } else {
        eprintln!("Failed to get event count from the file.");
    }
}

