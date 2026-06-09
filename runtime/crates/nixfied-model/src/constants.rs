pub const MODEL_VERSION: u32 = 1;
pub const TOOLCHAIN_ID: &str = "nixfied-toolchain:1";
pub const RUNTIME_ABI: &str = "nixfied-runtime-abi:1";

pub fn expected_model_version() -> u32 {
    MODEL_VERSION
}

pub fn expected_toolchain_id() -> &'static str {
    TOOLCHAIN_ID
}

pub fn expected_runtime_abi() -> &'static str {
    RUNTIME_ABI
}
