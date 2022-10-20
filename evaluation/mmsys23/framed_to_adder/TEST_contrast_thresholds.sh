#!/usr/bin/bash
##
##
## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
# sudo mkdir /mnt/tmp
## Ex mount the ram disk with 10 GB of RAM
# sudo mount -t tmpfs -o size=10g tmpfs /mnt/tmp



TICKS=255

./evaluate_framed_to_adder.sh \
  /media/andrew/ExternalM2/ugc-dataset/original_videos_h264 \
  ./dataset/contrast_thresholds_filelist.txt \
  /home/andrew/Documents/ADDER_10_20_FRAMED_RESULTS \
  "${TICKS}" \
  40 \
  /mnt/tmp
