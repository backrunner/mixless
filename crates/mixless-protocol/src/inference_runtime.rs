//! Pinned Apple Silicon plugin runtime, prepared at build/package time only.
pub const VERSION: &str = "ort-1.29.0-mlx-0.29.6";
pub const ORT_LIBRARY: &str = "libonnxruntime.1.29.0.dylib";
pub const MLX_LIBRARY: &str = "libonnxruntime_mlx_ep.dylib";
pub const MLX_PROVIDER: &str = "MLXExecutionProvider";

pub struct Archive {
    pub url: &'static str,
    pub sha256: &'static str,
    pub prefix: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

// Wheels are just checksum-verified ZIP containers here. No Python is shipped
// or invoked; only the native libraries and Metal kernels are extracted.
pub const ARCHIVES: &[Archive] = &[
    Archive {
        url: "https://files.pythonhosted.org/packages/7c/a8/0520890321b8ff40b908cf165a93eb58fbc8f85c14db637277ea866c9544/onnxruntime-1.29.0-cp311-cp311-macosx_14_0_arm64.whl",
        sha256: "07c5907474dec4a2792fd7626b753dc66707808385a6d9eecf993db0066a9d0f",
        prefix: "onnxruntime/capi/",
        files: &[(ORT_LIBRARY, "7af5534acfb76283a8734a6f8684617879446e98823f0144ea918438a15981e2")],
    },
    Archive {
        url: "https://files.pythonhosted.org/packages/ef/f2/6f0070bd74b6ea3cef52ecb8da2e821de044241a1f6fc6e4cb2195aa966e/onnxruntime_ep_mlx-0.29.6-py3-none-macosx_14_0_arm64.whl",
        sha256: "6ce838e9c39b799a122a6cf022c93dfb6c6f74540825acf49ece913621a7fda3",
        prefix: "onnxruntime_ep_mlx/",
        files: &[
            (MLX_LIBRARY, "4188ae83ec452b81bf198c7fdea56d739b105d11950544f751c74608277b5f7a"),
            ("libmlx.dylib", "52513f99c66c0a6f31a3b6b58436a8dfd5db5f0c30dbf2e37de309ec74f6c490"),
            ("libmlxc.dylib", "152930c03893fa96a0975214ba17597c8df59deb00b4b29c8df5b9b3e83a4ab1"),
            ("mlx.metallib", "266313bb90850db63dbbba74ba586d13f34427807a4c466aa1c8c6bd3ad5259d"),
        ],
    },
];
