#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

extern void wrapper_rust_emit_android_log(int prio, const char *tag, const char *message);

void wrapper_android_log_shim_anchor(void) {}

static const char *wrapper_android_log_string(const char *value) {
    return value != NULL ? value : "<null>";
}

static void wrapper_android_log_forward_message_v(
    int prio,
    const char *tag,
    const char *prefix,
    const char *fmt,
    va_list args
) {
    const char *resolved_tag = wrapper_android_log_string(tag);
    const char *resolved_prefix = prefix != NULL ? prefix : "";
    size_t prefix_len = strlen(resolved_prefix);
    int body_len = 0;
    size_t separator_len = 0;

    if (fmt != NULL) {
        va_list sizing_args;
        va_copy(sizing_args, args);
        body_len = vsnprintf(NULL, 0, fmt, sizing_args);
        va_end(sizing_args);
        if (body_len < 0) {
            wrapper_rust_emit_android_log(prio, resolved_tag, "<format-error>");
            return;
        }
        if (body_len > 0 && prefix_len > 0) {
            separator_len = 2;
        }
    }

    size_t total_len = prefix_len + separator_len + (size_t)body_len + 1;
    char stack_buffer[1024];
    char *buffer = stack_buffer;
    if (total_len > sizeof(stack_buffer)) {
        buffer = malloc(total_len);
        if (buffer == NULL) {
            wrapper_rust_emit_android_log(prio, resolved_tag, "<oom-formatting-android-log>");
            return;
        }
    }

    size_t offset = 0;
    if (prefix_len > 0) {
        memcpy(buffer, resolved_prefix, prefix_len);
        offset += prefix_len;
    }
    if (separator_len > 0) {
        buffer[offset++] = ':';
        buffer[offset++] = ' ';
    }
    if (body_len > 0) {
        vsnprintf(buffer + offset, total_len - offset, fmt, args);
    } else {
        buffer[offset] = '\0';
    }

    wrapper_rust_emit_android_log(prio, resolved_tag, buffer);
    if (buffer != stack_buffer) {
        free(buffer);
    }
}

int __android_log_print(int prio, const char *tag, const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    wrapper_android_log_forward_message_v(prio, tag, NULL, fmt, args);
    va_end(args);
    return 0;
}

int __android_log_write(int prio, const char *tag, const char *text) {
    wrapper_rust_emit_android_log(prio, wrapper_android_log_string(tag), wrapper_android_log_string(text));
    return 0;
}

void __android_log_assert(const char *cond, const char *tag, const char *fmt, ...) {
    char prefix[256];
    snprintf(
        prefix,
        sizeof(prefix),
        "assert failed (cond=%s)",
        wrapper_android_log_string(cond)
    );
    va_list args;
    va_start(args, fmt);
    wrapper_android_log_forward_message_v(7, tag, prefix, fmt, args);
    va_end(args);
    abort();
}

#ifdef __ANDROID__
void exit(int status) {
    dprintf(2, "[process_exit] exit(%d)\n", status);
    syscall(SYS_exit_group, status);
    __builtin_unreachable();
}

void _exit(int status) {
    dprintf(2, "[process_exit] _exit(%d)\n", status);
    syscall(SYS_exit_group, status);
    __builtin_unreachable();
}

void abort(void) {
    dprintf(2, "[process_exit] abort()\n");
    syscall(SYS_exit_group, 134);
    __builtin_unreachable();
}
#endif
