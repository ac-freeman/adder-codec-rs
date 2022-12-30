DATASET_PATH=$1   # e.g., /media/andrew/Scratch/ugc-dataset/original_videos_h264
FILELIST="./dataset/paper_figure_filelist.txt"   # e.g., ./evaluation/mmsys23/framed_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$2  # e.g., /media/andrew/Scratch1/ugc-dataset/evaluations
REF_TIME=255
MAX_THRESH=$3
DTM="$((${REF_TIME} * 120))"  # equivalent to 120 input frames of time, when ref_time is 5000
TEMP_DIR=$4
echo "${DTM}"
mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for f in "${!filenames[@]}"; do
    FILENAME="${filenames[f]}"
    echo "${FILENAME}"
    if [ ! -d "${DATA_LOG_PATH}/${FILENAME}" ]; then
        mkdir "${DATA_LOG_PATH}/${FILENAME}"
        for (( i = 0; i <= ${MAX_THRESH}; i += 10 ))
        do
            echo "${FILENAME}_${i}_${REF_TIME}"
            cargo run --release --bin adder_simulproc -- --color-input --scale 1.0 --input-filename "${DATASET_PATH}/${FILENAME}" --output-raw-video-filename "${TEMP_DIR}/tmp" --c-thresh-pos $i --c-thresh-neg $i --frame-count-max 0 --output-events-filename "${TEMP_DIR}/tmp_events.adder" --ref-time ${REF_TIME} --delta-t-max ${DTM} >> "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}.txt"
            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}.txt"
            rm -rf "${TEMP_DIR}/tmp"    # Delete the raw video data
            rm -rf "${TEMP_DIR}/tmp_events.adder"   # Delete the events file
            mv "${TEMP_DIR}/tmp.mp4" "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}.mp4"
        done
        sleep 5s
    fi
done