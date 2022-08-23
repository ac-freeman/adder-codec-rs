#!/usr/bin/bash
FOLDER="bunny"
cd ..

for (( i = 0; i <= 20; i += 2 ))
do 
#    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "/home/andrew/Downloads/waves/waves.mp4" --output-raw-video-filename "/home/andrew/Downloads/waves/out_${i}" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 0 --output-events-filename "/home/andrew/Downloads/waves/out_${i}.adder" --tps 150000 --delta-t-max 300000 --fps 30
    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "/home/andrew/Downloads/${FOLDER}/${FOLDER}.mp4" --output-raw-video-filename "/home/andrew/Downloads/${FOLDER}/out_${i}" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 1440 --output-events-filename "/home/andrew/Downloads/${FOLDER}/out_${i}.adder" --tps 120000 --delta-t-max 480000 --fps 24
    rm -rf "/home/andrew/Downloads/${FOLDER}/out_${i}"
#    ffmpeg -i "/home/andrew/Downloads/lake/out_${i}.mp4" -ss 00:00:00 -t 00:00:20.833333 -crf 0 -c:v libx264 -y "/home/andrew/Downloads/lake/out_${i}.mp4"
#    docker run -v /home/andrew/Downloads/lake:/vids gfdavila/easyvmaf -r /vids/out_${i}.mp4 -d "/vids/drop_scaled_hd_out_${i}".mp4
    docker run -v /home/andrew/Downloads/${FOLDER}:/vids gfdavila/easyvmaf -r /vids/${FOLDER}.mp4 -d /vids/out_${i}.mp4 -sw 1 -endsync


done 