FROM debian:stable-slim AS ffmpeg

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

FROM rust:slim-bookworm as builder
# Install android NDK
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl unzip \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/android-ndk \
    && curl -fsSL "https://dl.google.com/android/repository/android-ndk-r25c-linux.zip" -o /tmp/android-ndk.zip \
    && unzip /tmp/android-ndk.zip -d /opt/android-ndk --strip-components=1 \
    && rm -f /tmp/android-ndk.zip
RUN cargo install cargo-ndk

COPY . /app
WORKDIR /app
RUN ANDROID_NDK_HOME=/opt/android-ndk cargo ndk -t x86_64 --release --output ./target/wrapper


FROM debian:stable-slim as downloader

# download rootfs from https://github.com/WorldObservationLog/wrapper

FROM debian:stable-slim

ENV args=""

COPY --from=downloader /app/rootfs /app/rootfs
RUN mkdir -p /app/rootfs/usr/local/bin
COPY --from=ffmpeg /opt/ffmpeg/ffmpeg /app/rootfs/usr/local/bin/ffmpeg
COPY --from=ffmpeg /opt/ffmpeg/ffprobe /app/rootfs/usr/local/bin/ffprobe
COPY --from=builder /app/target/wrapper/wrapper /app/wrapper
WORKDIR /app

CMD ["bash", "-c", "/app/wrapper $args"]

EXPOSE 8080
