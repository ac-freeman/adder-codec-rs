#!/usr/bin/bash
## This script transcodes the DAVIS dataset to ADΔER at various ADΔER contrast thresholds, which remain fixed during
## the course of the transcode.

## Results are output in the form of a text log
## containing the execution time and results of `adderinfo`, and a json file containing the results of VMAF perceptual
## quality analysis of the framed reconstructions.

## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
 sudo mkdir /mnt/tmp
## Ex mount the ram disk with 20 GB of RAM
 sudo mkdir /mnt/tmp
## Ex mount the ram disk with 30 GB of RAM
 sudo mount -t tmpfs -o size=30g tmpfs /mnt/tmp

FILELIST=$1   # e.g., ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$2  # e.g., /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation

FPS=500.0

FRAMED_DAVIS_TO_ADDER_TO_DVS_PATH=/home/andrew/Documents/ADDER_10_31_FRAMED_DAVIS_40_RESULTS_wDVS
RAW_DAVIS_TO_ADDER_TO_DVS_PATH=/home/andrew/Documents/ADDER_11_2_RAW_DAVIS_40_RESULTS_wDVS
DVS_TO_ADDER_TO_DVS_PATH=/home/andrew/Documents/11_1_22_dvs_to_adder_evaluation
DVS_TO_DVS_FRAMED_PATH=/home/andrew/Documents/ADDER_11_2_dvs_to_dvsframed

mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for i in "${!filenames[@]}"; do
    FILENAME="${filenames[i]}"
    echo "${FILENAME}"
   if [ ! -d "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps" ]; then # TODO: re-enable
    # if [ true ]; then
        mkdir "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps"
        ffmpeg \
          -i "${FRAMED_DAVIS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_${FPS}fps"/"40_${FPS}fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi psnr=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_davis_to_adder_psnr.txt")
        ffmpeg \
          -i "${FRAMED_DAVIS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_${FPS}fps"/"40_${FPS}fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi ssim=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/framed_davis_to_adder_ssim.txt")

        ffmpeg \
          -i "${RAW_DAVIS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_${FPS}fps"/"40_${FPS}fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi psnr=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/raw_davis_to_adder_psnr.txt")
        ffmpeg \
          -i "${RAW_DAVIS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_${FPS}fps"/"40_${FPS}fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi ssim=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/raw_davis_to_adder_ssim.txt")


        ffmpeg \
          -i "${DVS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_0fps"/"40_0fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi psnr=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/raw_dvs_to_adder_psnr.txt")
        ffmpeg \
          -i "${DVS_TO_ADDER_TO_DVS_PATH}"/"${FILENAME}_0fps"/"40_0fps_dvs.mp4" \
          -i "${DVS_TO_DVS_FRAMED_PATH}"/"${FILENAME}_${FPS}fps"/dvs.mp4 \
          -lavfi ssim=stats_file=psnr_logfile.txt -f null -  \
          |& tee >(grep Parsed_ >> "${DATA_LOG_PATH}/${FILENAME}_${FPS}fps/raw_dvs_to_adder_ssim.txt")

   fi
done