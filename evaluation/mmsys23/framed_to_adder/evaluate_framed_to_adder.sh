#!/usr/bin/bash
## Example usage:
# ./evaluate_framed_to_adder.sh \
# /media/andrew/Scratch/ugc-dataset/original_videos_h264 \
# ./dataset/test_filelist.txt \
# /media/andrew/Scratch/ugc-dataset/evaluations \
# 5000 \
# 50


DATASET_PATH=$1   # e.g., /media/andrew/Scratch/ugc-dataset/original_videos_h264
FILELIST=$2   # e.g., ./evaluation/mmsys23/framed_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$3  # e.g., /media/andrew/Scratch1/ugc-dataset/evaluations
REF_TIME=$4
MAX_THRESH=$5
DTM="$((${REF_TIME} * 120))"  # equivalent to 120 input frames of time, when ref_time is 5000
TEMP_DIR=$6
echo "${DTM}"
mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for i in "${!filenames[@]}"; do
    FILENAME="${filenames[i]}"
    echo "${FILENAME}"
    for (( i = 0; i <= ${MAX_THRESH}; i += 10 ))
    do
        mkdir "${DATA_LOG_PATH}/${FILENAME}"
        echo "${FILENAME}_${i}_${REF_TIME}"
        cargo run --release --bin adder_simulproc -- --show-display 0 --scale 1.0 --input-filename "${DATASET_PATH}/${FILENAME}" --output-raw-video-filename "${TEMP_DIR}/tmp" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 0 --output-events-filename "${TEMP_DIR}/tmp_events.adder" --ref-time ${REF_TIME} --delta-t-max ${DTM} >> "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}.txt"
        cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}.txt"
        rm -rf "${TEMP_DIR}/tmp"    # Delete the raw video data
        rm -rf "${TEMP_DIR}/tmp_events.adder"   # Delete the events file
        docker run -v ${DATASET_PATH}:/gt_vids -v "$(pwd)":/gen_vids gfdavila/easyvmaf -r "/gt_vids/${FILENAME}" -d /gen_vids/tmp.mp4 -sw 0.0 -ss 0 -endsync
        rm -rf "${TEMP_DIR}/tmp.mp4"
        mv "${TEMP_DIR}/tmp_vmaf.json" "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}_vmaf.json"
    done
    echo "${FILENAME}"
done

