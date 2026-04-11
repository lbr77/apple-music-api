fn main() {
    println!("cargo:rerun-if-changed=cpp/callbacks.cpp");
    println!("cargo:rerun-if-changed=cpp/android_log_shim.c");

    let target = std::env::var("TARGET").expect("TARGET is not set");

    cc::Build::new()
        .cpp(true)
        .file("cpp/callbacks.cpp")
        .flag_if_supported("-std=c++17")
        .compile("wrapper_cpp_callbacks");

    cc::Build::new()
        .file("cpp/android_log_shim.c")
        .compile("wrapper_android_log_shim");

    println!("cargo:rustc-link-arg=-Wl,--export-dynamic");

    if target.contains("apple") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if target.contains("android") {
        println!("cargo:rustc-link-lib=dylib=c++_shared");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
