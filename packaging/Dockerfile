FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV NO_STRIP=true

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    file \
    git \
    libappindicator3-dev \
    libasound2-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.1-dev \
    librsvg2-dev \
    patchelf \
    pkg-config \
    python3 \
    wget \
    xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Ubuntu 24.04's archive Node.js is too old for the Vite/Rolldown toolchain.
# Remove any transitive archive installation before installing NodeSource's
# Node 20 LTS packages, avoiding libnode-dev file ownership conflicts.
RUN apt-get update \
    && apt-get remove -y nodejs libnode-dev || true
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && node --version \
    && node --version | grep -Eq '^v20\.' \
    && npm --version \
    && rm -rf /var/lib/apt/lists/*

# Pin toolchain installation to the normal Rustup/Node distribution channels;
# linuxdeploy and appimagetool are supplied by the CI image or mounted by the
# caller so their versions can be checksum-pinned by the release workflow.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH

WORKDIR /work/Flint
