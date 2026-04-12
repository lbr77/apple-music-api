#include <functional>

extern "C" {

using EndLeaseFn = void (*)(int);
using PlaybackErrorFn = void (*)(void *);

void *wrapper_make_end_lease_callback(EndLeaseFn cb) {
    return new std::function<void(int const &)>([cb](int const &value) { cb(value); });
}

void *wrapper_make_playback_error_callback(PlaybackErrorFn cb) {
    return new std::function<void(void *)>([cb](void *value) { cb(value); });
}

void wrapper_free_end_lease_callback(void *ptr) {
    delete static_cast<std::function<void(int const &)> *>(ptr);
}

void wrapper_free_playback_error_callback(void *ptr) {
    delete static_cast<std::function<void(void *)> *>(ptr);
}

}
