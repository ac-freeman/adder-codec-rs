#!/usr/bin/bash
## This script gets data for 10 videos in different categories, at various settings of TPF (ticks per frame).
## An ADDER transcoder contrast threshold of 0 is used to ensure that the outputs are of the highest quality.
## Perceptual quality is evaluated with VMAF.

for ticks in {10,50,150,200,250,251,252,253,254,255,256,257,258,259,260,300,400,500}
do
    ./evaluate_framed_to_adder.sh \
     /media/andrew/ExternalM2/ugc-dataset/original_videos_h264 \
     ./dataset/tpf_optimization_filelist.txt \
     /media/andrew/ExternalM2/ugc-dataset/optimize_evals \
     "${ticks}" \
     0 \
     .
done