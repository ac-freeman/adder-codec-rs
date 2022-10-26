#!/usr/bin/bash
## This script transcodes framed videos from selected videos of the UGC dataset to ADDER at 255 ticks (since sources
##  have 8-bit color), and a maximum ADDER contrast threshold of 40. Results are output in the form of a text log
## containing the execution time and results of `adderinfo`, and a json file containing the results of VMAF perceptual
## quality analysis of the framed reconstructions.
##
## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
# sudo mkdir /mnt/tmp
## Ex mount the ram disk with 40 GB of RAM
# sudo mount -t tmpfs -o size=40g tmpfs /mnt/tmp



TICKS=255

./evaluate_framed_to_adder.sh \
  /media/andrew/ExternalM2/ugc-dataset/original_videos_h264 \
  ./dataset/contrast_thresholds_filelist.txt \
  /home/andrew/Documents/ADDER_10_20_FRAMED_RESULTS \
  "${TICKS}" \
  40 \
  /mnt/tmp
