#!/usr/bin/env bash
# Builds tiny synthetic fixtures for the analysis tools under tests/test_media/.
# ffmpeg only; no network; each fixture is a few seconds and well under 1 MB.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p test_media
cd test_media

# --- scenes.mp4: red 2s -> blue 2s -> green 2s: two hard cuts to find -------
if [ ! -f scenes.mp4 ]; then
    ffmpeg -y -hide_banner -loglevel error \
        -f lavfi -i "color=c=red:size=320x240:rate=30:duration=2" \
        -f lavfi -i "color=c=blue:size=320x240:rate=30:duration=2" \
        -f lavfi -i "color=c=green:size=320x240:rate=30:duration=2" \
        -filter_complex "[0:v][1:v][2:v]concat=n=3:v=1:a=0[out]" \
        -map "[out]" -pix_fmt yuv420p scenes.mp4
fi

# --- beats.wav: a 120 BPM click track, 24 clicks over ~12 s -----------------
if [ ! -f beats.wav ]; then
    rm -f click.wav gap.wav list.txt
    ffmpeg -y -hide_banner -loglevel error \
        -f lavfi -i "sine=f=1000:d=0.05" -af "afade=t=out:st=0.01:d=0.04" click.wav
    ffmpeg -y -hide_banner -loglevel error -f lavfi -i "anullsrc=r=44100:cl=mono:d=0.45" gap.wav
    : > list.txt
    for _ in $(seq 1 24); do
        echo "file '$PWD/click.wav'" >> list.txt
        echo "file '$PWD/gap.wav'" >> list.txt
    done
    ffmpeg -y -hide_banner -loglevel error -f concat -safe 0 -i list.txt -ar 44100 -ac 1 beats.wav
    rm -f click.wav gap.wav list.txt
fi

# --- gaps.wav: tone 1s, silence 1s, tone 1s, silence 2s, tone 1s ------------
if [ ! -f gaps.wav ]; then
    ffmpeg -y -hide_banner -loglevel error \
        -f lavfi -i "sine=f=440:d=1" \
        -f lavfi -i "anullsrc=r=44100:cl=mono:d=1" \
        -f lavfi -i "sine=f=440:d=1" \
        -f lavfi -i "anullsrc=r=44100:cl=mono:d=2" \
        -f lavfi -i "sine=f=440:d=1" \
        -filter_complex "[0:a][1:a][2:a][3:a][4:a]concat=n=5:v=0:a=1[out]" \
        -map "[out]" -ar 44100 -ac 1 gaps.wav
fi

echo "fixtures ready: scenes.mp4, beats.wav, gaps.wav"
