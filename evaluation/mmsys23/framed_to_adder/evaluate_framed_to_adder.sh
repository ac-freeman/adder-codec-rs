#!/usr/bin/bash
## Example usage:
# ./evaluate_framed_to_adder.sh \
# /media/andrew/Scratch/ugc-dataset/original_videos_h264 \
# ./dataset/test_filelist.txt \
# /media/andrew/Scratch/ugc-dataset/evaluations


DTM=600000  # equivalent to 120 input frames of time, when ref_time is 5000
REF_TIME=5000
DATASET_PATH=$1   # e.g., /media/andrew/Scratch/ugc-dataset/original_videos_h264
FILELIST=$2   # e.g., ./evaluation/mmsys23/framed_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$3  # e.g., /media/andrew/Scratch1/ugc-dataset/evaluations

mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for i in "${!filenames[@]}"; do
    FILENAME="${filenames[i]}"
    echo "${FILENAME}"
    for (( i = 0; i <= 50; i += 10 ))
    do
        mkdir "${DATA_LOG_PATH}/${FILENAME}"
        echo "${FILENAME}_${i}"
        cargo run --release --bin adder_simulproc -- --show-display 0 --scale 1.0 --input-filename "${DATASET_PATH}/${FILENAME}" --output-raw-video-filename "./tmp" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 0 --output-events-filename "./tmp_events.adder" --ref-time ${REF_TIME} --delta-t-max ${DTM} >> "${DATA_LOG_PATH}/${FILENAME}/${i}.txt"
        cargo run --release --bin adderinfo -- -i "./tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}/${i}.txt"
        rm -rf "./tmp"    # Delete the raw video data
        rm -rf "./tmp_events.adder"   # Delete the events file
        docker run -v ${DATASET_PATH}:/gt_vids -v "$(pwd)":/gen_vids gfdavila/easyvmaf -r "/gt_vids/${FILENAME}" -d /gen_vids/tmp.mp4 -sw 0.0 -ss 0 -endsync
        rm -rf "./tmp.mp4"
        mv "./tmp_vmaf.json" "${DATA_LOG_PATH}/${FILENAME}/${i}_vmaf.json"
    done
    echo "${FILENAME}"
done

