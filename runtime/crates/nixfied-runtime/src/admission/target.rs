use nixfied_model::Model;

use crate::admission::AdmissionContext;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_target(
    model: &Model,
    loaded: &LoadedModel,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if model.target.system != context.host_system
        || model.target.closure_system != context.host_system
        || model.target.os != host_os()
        || model.target.arch != host_arch()
    {
        return Err(RuntimeError::new(
            ErrorCode::PlatformUnsupported,
            format!(
                "model target system={} os={} arch={} closureSystem={} does not match host system={} os={} arch={}",
                model.target.system,
                model.target.os,
                model.target.arch,
                model.target.closure_system,
                context.host_system,
                host_os(),
                host_arch()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    let caps = &model.target.required_runtime_capabilities;
    if !(caps.process_group && caps.tcp_port_ownership && caps.sqlite_wal) {
        return Err(RuntimeError::new(
            ErrorCode::PlatformUnsupported,
            "required runtime capabilities are not the capability set",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    Ok(())
}

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}
