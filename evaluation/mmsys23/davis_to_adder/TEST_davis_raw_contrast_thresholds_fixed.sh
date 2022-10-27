#!/usr/bin/bash
## This script transcodes the DAVIS dataset to ADDER at various ADDER contrast thresholds, which remain fixed during
## the course of the transcode.

## Results are output in the form of a text log
## containing the execution time and results of `adderinfo`, and a json file containing the results of VMAF perceptual
## quality analysis of the framed reconstructions.

## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
 sudo mkdir /mnt/tmp
## Ex mount the ram disk with 20 GB of RAM
 sudo mount -t tmpfs -o size=20g tmpfs /mnt/tmp


 ./evaluate_davis_raw_to_adder.sh \
    /media/andrew/ExternalM2/mmsys23_davis_dataset \
    ./dataset/dataset_filelist.txt \
    /media/andrew/ExternalM2/10_26_22_davis_raw_to_adder_evaluation \
    40 \
    /mnt/tmp
