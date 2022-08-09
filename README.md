# ADDER-codec-rs

Encoder/decoder for ADΔER (Address, Decimation, Δt Event Representation) streams. Currently, only implemented for raw (uncompressed) streams.

[crates.io page](https://crates.io/crates/adder-codec-rs)

### Usage


Encode a raw stream:
```
let mut stream: RawStream = Codec::new();
match stream.open_writer("/path/to/file") {
    Ok(_) => {}
    Err(e) => {panic!("{}", e)}
};
stream.encode_header(500, 200, 50000, 5000, 50000, 1);

let event: Event = Event {
        coord: Coord {
            x: 10,
            y: 30,
            c: None
        },
        d: 5,
        delta_t: 1000
    };
let events = vec![event, event, event]; // Encode three identical events, for brevity's sake
stream.encode_events(&events);
stream.close_writer();
```

Read a raw stream:

```
let mut stream: RawStream = Codec::new();
stream.open_reader(args.input_filename.as_str())?;
stream.decode_header();
match self.stream.decode_event() {
    Ok(event) => {
        // Do something with the event
    }
    Err(_) => panic!("Couldn't read event :("),
};
stream.close_reader();
```

