#!/usr/bin/bash

cd ..

for (( i = 0; i <= 20; i += 2 ))
do 
    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "./tests/samples/videos/drop_scaled_hd.mp4" --output-raw-video-filename "./tests/samples/videos/drop_scaled_hd_out" --c-thresh-pos $i --c-thresh-neg $i
    ffmpeg -i "./tests/samples/videos/drop_scaled_hd_out.mp4" -ss 00:00:00 -t 00:00:20.833333 -crf 0 -c:v libx264 -y "./tests/samples/videos/drop_scaled_hd_out_${i}.mp4"
    docker run -v `pwd`/tests/samples/videos:/vids gfdavila/easyvmaf -r /vids/drop_scaled_hd_trimmed.mp4 -d "/vids/drop_scaled_hd_out_${i}".mp4

done 