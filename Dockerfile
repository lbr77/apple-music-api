FROM debian:bookworm-slim AS ffmpeg

ARG FFMPEG_STATIC_URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/ffmpeg \
    && curl -fsSL "$FFMPEG_STATIC_URL" -o /tmp/ffmpeg.tar.xz \
    && tar -xJf /tmp/ffmpeg.tar.xz -C /opt/ffmpeg --strip-components=1 \
    && test -x /opt/ffmpeg/ffmpeg \
    && test -x /opt/ffmpeg/ffprobe \
    && rm -f /tmp/ffmpeg.tar.xz

FROM debian:bookworm-slim AS mp4box

ARG ANDROID_NDK_URL="https://dl.google.com/android/repository/android-ndk-r25c-linux.zip"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        unzip \
        git \
        build-essential \
        pkg-config \
        zlib1g-dev \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/android-ndk \
    && curl -fsSL "$ANDROID_NDK_URL" -o /tmp/android-ndk.zip \
    && unzip -q /tmp/android-ndk.zip -d /opt/android-ndk \
    && ndk_dir="$(find /opt/android-ndk -mindepth 1 -maxdepth 1 -type d | head -n 1)" \
    && mv "$ndk_dir" /opt/android-ndk/current \
    && rm -f /tmp/android-ndk.zip \
    && git clone --depth 1 https://github.com/gpac/gpac.git /tmp/gpac \
    && cd /tmp/gpac \
    && toolchain="/opt/android-ndk/current/toolchains/llvm/prebuilt/linux-x86_64" \
    && sysroot="$toolchain/sysroot" \
    && ./configure \
        --prefix=/opt/mp4box \
        --target-os=android \
        --cross-prefix="$toolchain/bin/" \
        --cc=x86_64-linux-android21-clang \
        --cxx=x86_64-linux-android21-clang++ \
        --extra-cflags="--sysroot=$sysroot" \
        --extra-ldflags="--sysroot=$sysroot" \
        --static-bin \
    && sed -i "s#^AR=.*#AR=$toolchain/bin/llvm-ar#" config.mak \
    && sed -i "s#^RANLIB=.*#RANLIB=$toolchain/bin/llvm-ranlib#" config.mak \
    && sed -i "s#^STRIP=.*#STRIP=$toolchain/bin/llvm-strip#" config.mak \
    && make -C src -j"$(nproc)" \
    && make -C applications/mp4box -j"$(nproc)" \
    && mkdir -p /opt/mp4box/bin \
    && cp -f /tmp/gpac/applications/mp4box/mp4box /opt/mp4box/bin/MP4Box \
    && test -x /opt/mp4box/bin/MP4Box \
    && ln -sf /opt/mp4box/bin/MP4Box /opt/mp4box/bin/mp4box \
    && rm -rf /tmp/gpac

FROM rust:bookworm AS builder

ARG ANDROID_NDK_URL="https://dl.google.com/android/repository/android-ndk-r25c-linux.zip"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl unzip \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/android-ndk \
    && curl -fsSL "$ANDROID_NDK_URL" -o /tmp/android-ndk.zip \
    && unzip -q /tmp/android-ndk.zip -d /opt/android-ndk \
    && ndk_dir="$(find /opt/android-ndk -mindepth 1 -maxdepth 1 -type d | head -n 1)" \
    && mv "$ndk_dir" /opt/android-ndk/current \
    && rm -f /tmp/android-ndk.zip

ENV ANDROID_NDK_HOME=/opt/android-ndk/current
RUN cargo install --locked cargo-ndk
RUN rustup target add x86_64-linux-android

WORKDIR /app
COPY . .
RUN cargo ndk -t x86_64 build --release --bin main

FROM debian:bookworm-slim AS rootfs

ARG ROOTFS_ARCHIVE_URL="https://github.com/WorldObservationLog/wrapper/archive/refs/heads/main.tar.gz"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /tmp/rootfs /out \
    && curl -fsSL "$ROOTFS_ARCHIVE_URL" -o /tmp/rootfs/rootfs.tar.gz \
    && tar -xzf /tmp/rootfs/rootfs.tar.gz -C /tmp/rootfs \
    && src_dir="$(find /tmp/rootfs -mindepth 2 -maxdepth 2 -type d -name rootfs | head -n 1)" \
    && test -n "$src_dir" \
    && cp -a "$src_dir" /out/rootfs

FROM scratch

COPY --from=rootfs /out/rootfs /
COPY --from=ffmpeg /opt/ffmpeg/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg /opt/ffmpeg/ffprobe /usr/local/bin/ffprobe
COPY --from=mp4box /opt/mp4box/bin/MP4Box /usr/local/bin/MP4Box
COPY --from=mp4box /opt/mp4box/bin/mp4box /usr/local/bin/mp4box
COPY --from=builder /app/target/x86_64-linux-android/release/main /main

EXPOSE 8080
ENTRYPOINT ["/main"]
