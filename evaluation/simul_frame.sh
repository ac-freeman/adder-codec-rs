for (( i = 0; i <= 30; i += 2 )) 
do 
    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "../tests/samples/videos/drop_scaled.mp4" --output-raw-video-filename "../tests/samples/videos/drop_scaled_out_${i}" --c-thresh-pos $i --c-thresh-neg $i
done 