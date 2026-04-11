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
COPY --from=builder /app/target/x86_64-linux-android/release/main /main

EXPOSE 8080
ENTRYPOINT ["/main"]
