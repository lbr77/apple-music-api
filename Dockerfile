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

ARG GPAC_APT_URI="https://dist.gpac.io/gpac/linux/debian"
ARG GPAC_APT_COMPONENT="nightly"

RUN apt-get update \
    && set -eux \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        binutils \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://dist.gpac.io/gpac/linux/gpg.asc -o /etc/apt/keyrings/gpac.asc \
    && chmod a+r /etc/apt/keyrings/gpac.asc \
    && printf 'Types: deb\nURIs: %s\nSuites: %s\nComponents: %s\nSigned-By: /etc/apt/keyrings/gpac.asc\n' \
        "$GPAC_APT_URI" \
        "$(. /etc/os-release && echo "$VERSION_CODENAME")" \
        "$GPAC_APT_COMPONENT" \
        > /etc/apt/sources.list.d/gpac.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends gpac \
    && rm -rf /var/lib/apt/lists/* 

RUN mp4box_path="$(command -v MP4Box || command -v mp4box)" \
    && test -x "$mp4box_path" \
    && libgpac_path="$(find /usr/lib /lib -name 'libgpac.so*' ! -name '*.a' | sort | head -n 1)" \
    && test -n "$libgpac_path" \
    && test -e "$libgpac_path" \
    && interp="$(readelf -l "$mp4box_path" | sed -n 's/.*Requesting program interpreter: \(.*\)]/\1/p')" \
    && test -n "$interp" \
    && module_paths="$(if test -d /usr/lib/gpac; then find /usr/lib/gpac -type f -name '*.so' | sort; fi)" \
    && deps="$( \
        { \
            ldd "$mp4box_path" "$libgpac_path"; \
            if test -n "$module_paths"; then ldd $module_paths; fi; \
        } | awk ' \
                /=> \// { print $3 } \
                /^\// && $1 !~ /:$/ { print $1 } \
            ' \
          | sort -u \
    )" \
    && install -d /out/usr/bin /out/usr/lib /out/lib64 /out/etc/ssl/certs \
    && ln -s usr/lib /out/lib \
    && cp -a "$mp4box_path" /out/usr/bin/MP4Box \
    && if test -e /usr/bin/mp4box; then cp -a /usr/bin/mp4box /out/usr/bin/mp4box; fi \
    && if test -d /usr/lib/gpac; then cp -a /usr/lib/gpac /out/usr/lib/gpac; fi \
    && for dep in "$interp" "$libgpac_path" $deps; do \
        clean_dep="${dep%:}"; \
        real="$(readlink -f "$clean_dep")"; \
        install -d "/out$(dirname "$real")"; \
        cp -a "$real" "/out$real"; \
        if test -L "$clean_dep"; then \
            case "$clean_dep" in \
                /lib/*) \
                    link_path="/usr$clean_dep"; \
                    ;; \
                *) \
                    link_path="$clean_dep"; \
                    ;; \
            esac; \
            install -d "/out$(dirname "$link_path")"; \
            cp -a "$clean_dep" "/out$link_path"; \
        fi; \
    done \
    && for alt in \
        /etc/alternatives/libblas.so.3-x86_64-linux-gnu \
        /etc/alternatives/liblapack.so.3-x86_64-linux-gnu; do \
        if test -L "$alt"; then \
            alt_target="$(readlink "$alt")"; \
            install -d "/out$(dirname "$alt")" "/out$(dirname "$alt_target")"; \
            cp -a "$alt" "/out$alt"; \
            cp -a "$alt_target" "/out$alt_target"; \
        fi; \
    done \
    && if test -f /etc/ssl/certs/ca-certificates.crt; then \
        cp -a /etc/ssl/certs/ca-certificates.crt /out/etc/ssl/certs/ca-certificates.crt; \
    fi
    
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

# Android's dynamic loader must stay executable. Containerd refuses to start
# `/main` when the archived rootfs ships `system/bin/linker64` without the
# execute bit.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /tmp/rootfs /out \
    && curl -fsSL "$ROOTFS_ARCHIVE_URL" -o /tmp/rootfs/rootfs.tar.gz \
    && tar -xzf /tmp/rootfs/rootfs.tar.gz -C /tmp/rootfs \
    && src_dir="$(find /tmp/rootfs -mindepth 2 -maxdepth 2 -type d -name rootfs | head -n 1)" \
    && test -n "$src_dir" \
    && cp -a "$src_dir" /out/rootfs \
    && chmod 0755 /out/rootfs/system/bin/linker64

FROM scratch

COPY --from=rootfs /out/rootfs /
COPY --from=ffmpeg /opt/ffmpeg/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg /opt/ffmpeg/ffprobe /usr/local/bin/ffprobe
COPY --from=mp4box /out/ /
COPY --from=builder /app/target/x86_64-linux-android/release/main /main

EXPOSE 8080
ENTRYPOINT ["/main"]
