fn main() {
    println!("cargo:rerun-if-changed=native/unity_bridge.cpp");
    println!("cargo:rerun-if-changed=native/unity_bridge.h");
    println!("cargo:rerun-if-changed=../../include/dofus_native_mod_api.h");
    println!("cargo:rerun-if-changed=../../third_party/unityresolve/UnityResolve.hpp");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("native/unity_bridge.cpp")
        .include("native")
        .include("../../third_party/unityresolve")
        .define("NOMINMAX", None)
        .flag_if_supported("/std:c++20")
        .flag_if_supported("/EHsc")
        .warnings(false)
        .compile("dnh_unity_bridge");
}
