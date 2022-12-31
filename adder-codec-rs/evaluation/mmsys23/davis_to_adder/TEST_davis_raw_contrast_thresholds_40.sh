#!/usr/bin/bash
## This script transcodes the DAVIS dataset to ADΔER at various ADΔER contrast thresholds, which remain fixed during
## the course of the transcode.

## Results are output in the form of a text log
## containing the execution time and results of `adderinfo`, and a json file containing the results of VMAF perceptual
## quality analysis of the framed reconstructions.

## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
 sudo mkdir /mnt/tmp
## Ex mount the ram disk with 30 GB of RAM
 sudo mount -t tmpfs -o size=30g tmpfs /mnt/tmp

for fps in {50.0,75.0,100.0,250.0,500.0}
do
 ./evaluate_davis_raw_to_adder_40.sh \
    /media/andrew/ExternalM2/mmsys23_davis_dataset \
    ./dataset/dataset_filelist.txt \
    /home/andrew/Documents/ADDER_11_2_RAW_DAVIS_40_RESULTS \
    40 \
    "${fps}" \
    /mnt/tmp
done