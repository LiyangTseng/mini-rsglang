#!/bin/sh
# A fake third-party bench tool: sleeps briefly, then copies a canned
# result file into the directory/filename the harness asked it to write
# to. Mirrors the "renders argv, spawns, reads a result file" shape of
# `vllm bench serve --save-result` / `sglang.benchmark.serving
# --output-file` without actually invoking either tool (T-07-16: the
# harness never installs anything).
set -eu
sleep 0.2
cp "$3" "$1/$2"
