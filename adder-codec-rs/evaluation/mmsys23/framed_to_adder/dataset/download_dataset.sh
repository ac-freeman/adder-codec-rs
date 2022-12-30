#!/usr/bin/bash
## Download the Youtube UGC Datset
## (https://media.withyoutube.com/)

## Usage:
# download_dataset.txt /path/to/store/dataset

## Example usage:
# ./download_dataset.txt /media/andrew/Scratch/ugc-dataset

# Need to first install gsutil
# Ubuntu 20.04: sudo snap install google-cloud-cli --classic
DATASET_STORE_PATH=$1   # Where to store the dataset locally
mkdir -p "$DATASET_STORE_PATH";   # Make the directory, if it doesn't exist
gsutil cp -r gs://ugc-dataset/original_videos_h264 "$DATASET_STORE_PATH"    # Download the dataset with gsutil