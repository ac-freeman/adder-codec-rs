#!/usr/bin/bash
## Transcode an aedat4 file to ADΔER

## Example usage:
# ./evaluation/mmsys23/davis_to_adder/evaluate_davis_raw_to_adder.sh /media/andrew/ExternalM2/DynamicVision ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation 40


DATASET_PATH=$1   # e.g., /media/andrew/ExternalM2/DynamicVision
FILELIST="./dataset/paper_figure_filelist.txt"   # e.g., ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$2  # e.g., /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation
REF_TIME=1000000  # match the temporal granularity of the camera (microseconds)
MAX_THRESH=40
FPS=500
DTM="$((1000000 * 4))"  # 4 seconds
TEMP_DIR=$3

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

            cd /home/andrew/Code/davis-EDI-rs
            cargo run --release -- --args-filename "" \
                                           --base-path "${DATASET_PATH}" \
                                           --mode "file" \
                                           --events-filename-0 "${FILENAME}" \
                                           --start-c 0.30344322344322345 \
                                           --optimize-c \
                                           --simulate-packet-latency \
                                           --target-latency 1000.0 \
                                           --output-fps ${FPS} \
                                           --write-video \
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_.txt"
            mv "${TEMP_DIR}/output_file.mp4" "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon.mp4"


            ################# DAVIS framed reconstruction to ADDER
            cd /home/andrew/Code/adder-codec-rs
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
                                           deblur_only = false
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
                --transcode-from "framed" \
                --write-out \
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"

            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"
            cargo run --release --example events_to_instantaneous_frames >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"
            ffmpeg -f rawvideo -pix_fmt gray -s:v 346x260 -r 30 -i "/mnt/tmp/temppp_out" \
                                -crf 0 -c:v libx264 -y "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.mp4"

            cargo run --release --bin adder_to_dvs -- -i "${TEMP_DIR}/tmp_events.adder" \
                                --output-text "${TEMP_DIR}/dvs.txt" \
                                --output-video "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder_to_dvs.mp4" \
                                --fps 500.0 >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"
            wc -l "${TEMP_DIR}/dvs.txt" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt" # Print number of DVS events
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt"
            wc -c "${TEMP_DIR}/tmp_events.adder" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_recon_to_adder.txt" # Print size in bytes of ADDER file



            ################# DAVIS raw to ADDER
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
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"

            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"
            cargo run --release --example events_to_instantaneous_frames >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"
            ffmpeg -f rawvideo -pix_fmt gray -s:v 346x260 -r 30 -i "/mnt/tmp/temppp_out" \
                                -crf 0 -c:v libx264 -y "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.mp4"

            cargo run --release --bin adder_to_dvs -- -i "${TEMP_DIR}/tmp_events.adder" \
                                --output-text "${TEMP_DIR}/dvs.txt" \
                                --output-video "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder_to_dvs.mp4" \
                                --fps 500.0 >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"
            wc -l "${TEMP_DIR}/dvs.txt" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt" # Print number of DVS events
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt"
            wc -c "${TEMP_DIR}/tmp_events.adder" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/davis_raw_to_adder.txt" # Print size in bytes of ADDER file



            ################# DVS raw to ADDER
            cargo run --bin davis_to_adder --release -- \
              --edi-args "
                                           args_filename = \"\"
                                           base_path = \"${DATASET_PATH}\"
                                           mode = \"file\"
                                           events_filename_0 = \"${FILENAME}\"
                                           events_filename_1 = \"\"
                                           start_c = 0.30344322344322345
                                           optimize_c = false
                                           optimize_controller = false
                                           deblur_only = true
                                           events_only = true
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
                --transcode-from "raw-dvs" \
                --write-out \
                >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"

            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"
            cargo run --release --example events_to_instantaneous_frames >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"
            ffmpeg -f rawvideo -pix_fmt gray -s:v 346x260 -r 30 -i "/mnt/tmp/temppp_out" \
                                -crf 0 -c:v libx264 -y "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.mp4"

            cargo run --release --bin adder_to_dvs -- -i "${TEMP_DIR}/tmp_events.adder" \
                                --output-text "${TEMP_DIR}/dvs.txt" \
                                --output-video "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder_to_dvs.mp4" \
                                --fps 500.0 >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"
            wc -l "${TEMP_DIR}/dvs.txt" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt" # Print number of DVS events
            echo -e "\n" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt"
            wc -c "${TEMP_DIR}/tmp_events.adder" >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/dvs_raw_to_adder.txt" # Print size in bytes of ADDER file


        done
        sleep 5s
    fi
done

