#!/usr/bin/bash
## Transcode an aedat4 file to ADΔER

## Example usage:
# ./evaluation/mmsys23/davis_to_adder/evaluate_dvs_to_adder.sh /media/andrew/ExternalM2/DynamicVision ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation 40


DATASET_PATH=$1   # e.g., /media/andrew/ExternalM2/DynamicVision
FILELIST=$2   # e.g., ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$3  # e.g., /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation
REF_TIME=1000000  # match the temporal granularity of the camera (microseconds)
FPS=$4
DTM="$((1000000 * 4))"  # 4 seconds

echo "${DTM}"
mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for f in "${!filenames[@]}"; do
    FILENAME="${filenames[f]}"
    echo "${FILENAME}"
   if [ ! -d "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps" ]; then # TODO: re-enable
    # if [ true ]; then
        mkdir "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps"

            echo "${FILENAME}_${i}_${FPS}"
            cargo run --bin aedat4_dvs_visualize --release -- \
             --input "${DATASET_PATH}/${FILENAME}" \
             --output-video "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs.mp4" \
             --fps "${FPS}" \
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/0_${FPS}fps.txt"
        sleep 5s
    fi
done

