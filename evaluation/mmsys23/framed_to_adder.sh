#!/usr/bin/bash
FOLDER="waves"
SOURCE_FPS=30.0
#SOURCE_START_FRAME=$(bc -l <<< "1/${SOURCE_FPS}")
SOURCE_START_FRAME=0
DTM=480000
TPS=120000

cd ..

for (( i = 0; i <= 40; i += 5 ))
do 
    cargo run --release --bin transcode_and_frame_simultaneous -- --show-display 0 --scale 1.0 --input-filename "/home/andrew/Downloads/${FOLDER}/${FOLDER}.mp4" --output-raw-video-filename "/home/andrew/Downloads/${FOLDER}/out_${i}" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 1440 --output-events-filename "/home/andrew/Downloads/${FOLDER}/out_${i}.adder" --tps ${TPS} --delta-t-max ${DTM} --fps ${SOURCE_FPS}
    rm -rf "/home/andrew/Downloads/${FOLDER}/out_${i}"
    docker run -v /home/andrew/Downloads/${FOLDER}:/vids gfdavila/easyvmaf -r /vids/${FOLDER}.mp4 -d /vids/out_${i}.mp4 -sw 0.0 -ss $SOURCE_START_FRAME -endsync
#    cargo run --bin adderinfo --release -- -i /home/andrew/Downloads/${FOLDER}/out_${i}.adder -d
done