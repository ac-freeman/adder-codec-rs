#!/usr/bin/bash
# Need to first install gsutil
# Ubuntu 20.04: sudo snap install google-cloud-cli --classic
DATASET_STORE_PATH=$1   # Where to store the dataset locally
mkdir -p "$DATASET_STORE_PATH";   # Make the directory, if it doesn't exist
gsutil cp -r gs://ugc-dataset/original_videos_h264 "$DATASET_STORE_PATH"    # Download the dataset with gsutil