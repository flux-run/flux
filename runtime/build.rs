fn main() {
    let protoc_path = protoc_bin_vendored::protoc_bin_path()
        .expect("failed to fetch vendored protoc binary");
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&["../server/proto/internal_auth.proto"], &["../server/proto"])
        .expect("failed to compile gRPC protos for runtime");
}
