#!/usr/bin/env bash
# Compila o llama.cpp (commit fixado) para x86_64 e para armv7 (musl, estático) com o RPC, aplicando
# armv7.patch. Saída: volume Docker aic-llama (binários em /work/llama.cpp/build-*/bin).
#
#   bash code/third_party/llama.cpp/build.sh
#
# O armv7 usa zig (imagem aic-rust-zig:1, veja code/scripts/build-armv7.sh) com musl, a mesma base
# do worker. O x86 sai sem -march=native, para PCs antigos.
set -euo pipefail
cd "$(dirname "$0")"
COMMIT=8ca4060b7e30e76baaa5670ee1ec1b760b7f8552
docker image inspect aic-rust-zig:1 >/dev/null 2>&1 || { echo "falta a imagem aic-rust-zig:1 (rode code/scripts/build-armv7.sh)" >&2; exit 2; }
MSYS_NO_PATHCONV=1 docker run --rm -v aic-llama:/work -v "$(pwd -W 2>/dev/null || pwd):/p:ro" -w /work aic-rust-zig:1 bash -c "
  set -e
  apt-get update -qq >/dev/null && apt-get install -y -qq cmake git >/dev/null
  [ -d llama.cpp ] || git clone -q https://github.com/ggml-org/llama.cpp
  cd llama.cpp && git fetch -q origin $COMMIT && git checkout -q -f $COMMIT
  git apply --check /p/armv7.patch && git apply /p/armv7.patch
  # x86_64
  cmake -B build-x86 -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DGGML_RPC=ON -DLLAMA_CURL=OFF \
    -DBUILD_SHARED_LIBS=OFF -DLLAMA_BUILD_TESTS=OFF >/dev/null
  cmake --build build-x86 -j\$(nproc) --target llama-server llama-bench ggml-rpc-server
  # armv7 musl (Cortex-A7), via zig
  printf '#!/bin/sh\nexec python3 -m ziglang cc -target arm-linux-musleabihf -mcpu=cortex_a7 \"\$@\"\n' > /work/zigcc
  printf '#!/bin/sh\nexec python3 -m ziglang c++ -target arm-linux-musleabihf -mcpu=cortex_a7 \"\$@\"\n' > /work/zigcxx
  printf '#!/bin/sh\nexec python3 -m ziglang ar \"\$@\"\n' > /work/zigar
  printf '#!/bin/sh\nexec python3 -m ziglang ranlib \"\$@\"\n' > /work/zigranlib
  chmod +x /work/zig*
  cmake -B build-armv7m -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=arm \
    -DCMAKE_C_COMPILER=/work/zigcc -DCMAKE_CXX_COMPILER=/work/zigcxx -DCMAKE_AR=/work/zigar \
    -DCMAKE_RANLIB=/work/zigranlib -DCMAKE_EXE_LINKER_FLAGS=-static -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_RPC=ON -DLLAMA_CURL=OFF -DBUILD_SHARED_LIBS=OFF \
    -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF >/dev/null
  cmake --build build-armv7m -j\$(nproc) --target llama-bench ggml-rpc-server
  # x86_64 musl estático (rpc-server da imagem x86 do SO, Alpine), via zig
  printf '#!/bin/sh\nexec python3 -m ziglang cc -target x86_64-linux-musl -mcpu=x86_64 \"\$@\"\n' > /work/zigcc64
  printf '#!/bin/sh\nexec python3 -m ziglang c++ -target x86_64-linux-musl -mcpu=x86_64 \"\$@\"\n' > /work/zigcxx64
  chmod +x /work/zigcc64 /work/zigcxx64
  cmake -B build-x86m -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
    -DCMAKE_C_COMPILER=/work/zigcc64 -DCMAKE_CXX_COMPILER=/work/zigcxx64 -DCMAKE_AR=/work/zigar \
    -DCMAKE_RANLIB=/work/zigranlib -DCMAKE_EXE_LINKER_FLAGS=-static -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_RPC=ON -DLLAMA_CURL=OFF -DBUILD_SHARED_LIBS=OFF \
    -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF >/dev/null
  cmake --build build-x86m -j\$(nproc) --target ggml-rpc-server
  ls -l build-x86/bin build-armv7m/bin build-x86m/bin
"
