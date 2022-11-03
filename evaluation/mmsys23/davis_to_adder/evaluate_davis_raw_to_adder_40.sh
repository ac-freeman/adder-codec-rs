#!/usr/bin/bash
## Transcode an aedat4 file to ADΔER

## Example usage:
# ./evaluation/mmsys23/davis_to_adder/evaluate_davis_raw_to_adder.sh /media/andrew/ExternalM2/DynamicVision ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation 40


DATASET_PATH=$1   # e.g., /media/andrew/ExternalM2/DynamicVision
FILELIST=$2   # e.g., ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$3  # e.g., /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation
REF_TIME=1000000  # match the temporal granularity of the camera (microseconds)
MAX_THRESH=$4
FPS=$5
DTM="$((1000000 * 4))"  # 4 seconds
TEMP_DIR=$6

echo "${DTM}"
mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for f in "${!filenames[@]}"; do
    FILENAME="${filenames[f]}"
    echo "${FILENAME}"
   if [ ! -d "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps" ]; then # TODO: re-enable
    # if [ true ]; then
        mkdir "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps"
        for (( i = 40; i <= ${MAX_THRESH}; i += 10 ))
        do
            echo "${FILENAME}_${i}_${FPS}fps"
            cargo run --bin davis_to_adder --release -- \
              --edi-args "
                                           args_filename = \"\"
                                           base_path = \"${DATASET_PATH}\"
                                           mode = \"file\"
                                           events_filename_0 = \"${FILENAME}\"
                                           events_filename_1 = \"\"
                                           start_c = 0.30344322344322345
                                           optimize_c = true
                                           optimize_controller = false
                                           deblur_only = true
                                           events_only = false
                                           simulate_packet_latency = true
                                           target_latency = 1000.0
                                           show_display = false
                                           show_blurred_display = false
                                           output_fps = ${FPS}
                                           write_video = false" \
                --args-filename "" \
                --output-events-filename "${TEMP_DIR}/tmp_events.adder" \
                --adder-c-thresh-pos "${i}" \
                --adder-c-thresh-neg "${i}" \
                --delta-t-max-multiplier 4.0 \
                --transcode-from "raw-davis" \
                --write-out \
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt"
#                --show-display
#                --optimize-adder-controller  # Disabled so that the adder contrast threshold remains constant


            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt"


            cargo run --release --bin adder_to_dvs -- -i "${TEMP_DIR}/tmp_events.adder" \
                                --output-text "${TEMP_DIR}/dvs.txt" \
                                --output-video "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps_dvs.mp4" \
                                --fps 500.0 >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt"
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt"
            wc -l "${TEMP_DIR}/dvs.txt" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt" # Print number of DVS events
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt"
            wc -c "${TEMP_DIR}/tmp_events.adder" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/${i}_${FPS}fps.txt" # Print size in bytes of ADDER file

            

            




#            rm -rf "${TEMP_DIR}/tmp_events.adder"   # Delete the events file
#            docker run -v ${DATASET_PATH}:/gt_vids -v "${TEMP_DIR}":/gen_vids gfdavila/easyvmaf -r "/gt_vids/${FILENAME}" -d /gen_vids/tmp.mp4 -sw 0.0 -ss 0 -endsync
#            rm -rf "${TEMP_DIR}/tmp.mp4"
#            mv "${TEMP_DIR}/tmp_vmaf.json" "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}_vmaf.json"
        done
        sleep 5s
    fi
done

