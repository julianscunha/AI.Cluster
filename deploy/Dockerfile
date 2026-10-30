# Imagem única do AI Cluster para x86_64: controller, gateway, worker, CLI e o llama.cpp (llama-server
# e rpc-server) já com o patch do projeto. Contexto: a raiz do repositório.
#
#   docker build -f deploy/Dockerfile -t ai-cluster:local .

FROM rust:1-bookworm AS rust
WORKDIR /src
COPY code/ code/
RUN cd code && cargo build --release -p ai-cluster-controller -p ai-cluster-gateway \
      -p ai-cluster-worker -p ai-cluster-cli

FROM debian:bookworm AS llama
RUN apt-get update -qq && apt-get install -y -qq build-essential cmake git ca-certificates >/dev/null
# O commit tem de ser o de code/third_party/llama.cpp/build.sh: o protocolo do RPC muda entre commits.
ARG LLAMA_COMMIT=8ca4060b7e30e76baaa5670ee1ec1b760b7f8552
RUN git clone -q https://github.com/ggml-org/llama.cpp /llama \
 && cd /llama && git checkout -q $LLAMA_COMMIT
COPY code/third_party/llama.cpp/armv7.patch /armv7.patch
# Sem -march=native: o binário roda em PCs antigos.
RUN cd /llama && git apply /armv7.patch \
 && cmake -B build -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DGGML_RPC=ON -DLLAMA_CURL=OFF \
      -DBUILD_SHARED_LIBS=OFF -DLLAMA_BUILD_TESTS=OFF >/dev/null \
 && cmake --build build -j"$(nproc)" --target llama-server ggml-rpc-server

FROM debian:bookworm-slim
RUN apt-get update -qq && apt-get install -y -qq libgomp1 ca-certificates curl openssl >/dev/null \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin aic \
 && mkdir /cache && chown aic /cache
COPY --from=rust /src/code/target/release/ai-cluster /src/code/target/release/ai-cluster-controller \
     /src/code/target/release/ai-cluster-gateway /src/code/target/release/ai-cluster-worker /usr/local/bin/
COPY --from=llama /llama/build/bin/llama-server /llama/build/bin/ggml-rpc-server /usr/local/bin/
USER aic
