#!/usr/bin/bash

cd ..

for (( i = 0; i <= 20; i += 2 ))
do 
    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "/home/andrew/Downloads/lake_scaled_hd.mp4" --output-raw-video-filename "/home/andrew/Downloads/lake/out_${i}" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 250
#    ffmpeg -i "/home/andrew/Downloads/lake/out_${i}.mp4" -ss 00:00:00 -t 00:00:20.833333 -crf 0 -c:v libx264 -y "/home/andrew/Downloads/lake/out_${i}.mp4"
#    docker run -v /home/andrew/Downloads/lake:/vids gfdavila/easyvmaf -r /vids/out_${i}.mp4 -d "/vids/drop_scaled_hd_out_${i}".mp4

done 