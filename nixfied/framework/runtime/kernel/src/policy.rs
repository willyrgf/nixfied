use super::*;

pub(crate) fn service_policy_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "runtime-event" => service_policy_runtime_event_command(values),
        "start-service" => service_policy_start_service_command(values),
        "fixture-keep-running" => service_policy_fixture_keep_running_command(values),
        other => Err(format!("unknown service-policy subcommand: {}", other)),
    }
}

pub(crate) fn service_policy_runtime_event_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel service-policy runtime-event <reuse-policy|empty> <owner-scope|empty> <discovery-scope|empty> <ephemeral-flag> <export-file>"
                .to_string(),
        );
    }

    let explicit_reuse = values[0].as_str();
    let explicit_owner = values[1].as_str();
    let explicit_discovery = values[2].as_str();
    let ephemeral = parse_bool_flag(&values[3])?;
    let resolved_owner = if !explicit_owner.is_empty() {
        explicit_owner.to_string()
    } else {
        let from_reuse = service_policy_owner_scope_from_reuse(explicit_reuse);
        if !from_reuse.is_empty() {
            from_reuse.to_string()
        } else if ephemeral {
            "ephemeral".to_string()
        } else {
            "persistent".to_string()
        }
    };
    let resolved_discovery = if !explicit_discovery.is_empty() {
        explicit_discovery.to_string()
    } else {
        let from_reuse = service_policy_discovery_scope_from_reuse(explicit_reuse);
        if !from_reuse.is_empty() {
            from_reuse.to_string()
        } else if ephemeral {
            "local".to_string()
        } else {
            "global".to_string()
        }
    };
    let resolved_reuse = service_policy_infer_reuse_policy(
        explicit_reuse,
        &resolved_owner,
        &resolved_discovery,
        "same-slot",
    );

    validate_service_policy_matrix(
        &resolved_reuse,
        &resolved_owner,
        &resolved_discovery,
        false,
        false,
    )?;
    write_shell_exports(
        &values[4],
        &[
            ("OWNER_SCOPE".to_string(), resolved_owner),
            ("DISCOVERY_SCOPE".to_string(), resolved_discovery),
            ("REUSE_POLICY".to_string(), resolved_reuse),
        ],
    )?;
    println!("OK: service-policy runtime-event");
    Ok(())
}

pub(crate) fn service_policy_start_service_command(values: &[String]) -> Result<(), String> {
    if values.len() != 4 {
        return Err(
            "usage: nixfied-kernel service-policy start-service <reuse-policy|empty> <owner-scope|empty> <discovery-scope|empty> <export-file>"
                .to_string(),
        );
    }

    let explicit_reuse = values[0].as_str();
    let explicit_owner = values[1].as_str();
    let explicit_discovery = values[2].as_str();
    let resolved_owner = if !explicit_owner.is_empty() {
        explicit_owner.to_string()
    } else {
        let from_reuse = service_policy_owner_scope_from_reuse(explicit_reuse);
        if !from_reuse.is_empty() {
            from_reuse.to_string()
        } else {
            service_policy_owner_scope_from_discovery(explicit_discovery).to_string()
        }
    };
    let resolved_discovery = if !explicit_discovery.is_empty() {
        explicit_discovery.to_string()
    } else {
        let from_reuse = service_policy_discovery_scope_from_reuse(explicit_reuse);
        if !from_reuse.is_empty() {
            from_reuse.to_string()
        } else {
            service_policy_discovery_scope_from_owner(&resolved_owner).to_string()
        }
    };
    let resolved_reuse =
        service_policy_infer_reuse_policy(explicit_reuse, &resolved_owner, &resolved_discovery, "");
    validate_service_policy_matrix(
        &resolved_reuse,
        &resolved_owner,
        &resolved_discovery,
        true,
        true,
    )?;
    let register_cleanup = service_policy_start_service_register_cleanup(
        &resolved_reuse,
        &resolved_owner,
        &resolved_discovery,
    )?;
    write_shell_exports(
        &values[3],
        &[
            ("OWNER_SCOPE".to_string(), resolved_owner),
            ("DISCOVERY_SCOPE".to_string(), resolved_discovery),
            ("REUSE_POLICY".to_string(), resolved_reuse),
            ("REGISTER_CLEANUP".to_string(), register_cleanup),
        ],
    )?;
    println!("OK: service-policy start-service");
    Ok(())
}

pub(crate) fn service_policy_fixture_keep_running_command(values: &[String]) -> Result<(), String> {
    if values.len() != 4 {
        return Err(
            "usage: nixfied-kernel service-policy fixture-keep-running <owner-scope|empty> <reuse-policy|empty> <discovery-scope|empty> <export-file>"
                .to_string(),
        );
    }

    let owner_scope = values[0].as_str();
    let reuse_policy = values[1].as_str();
    let discovery_scope = values[2].as_str();
    let keep_running =
        service_policy_fixture_keep_running(owner_scope, reuse_policy, discovery_scope)?;

    write_shell_exports(&values[3], &[("KEEP_RUNNING".to_string(), keep_running)])?;
    println!("OK: service-policy fixture-keep-running");
    Ok(())
}

pub(crate) fn service_policy_owner_scope_from_reuse(reuse: &str) -> &'static str {
    match reuse {
        "same-slot" | "cross-run" => "persistent",
        "same-root" => "ephemeral",
        _ => "",
    }
}

pub(crate) fn service_policy_owner_scope_from_discovery(discovery: &str) -> &'static str {
    match discovery {
        "global" => "persistent",
        "local" => "ephemeral",
        _ => "",
    }
}

pub(crate) fn service_policy_discovery_scope_from_reuse(reuse: &str) -> &'static str {
    match reuse {
        "same-slot" | "cross-run" => "global",
        "same-root" => "local",
        _ => "",
    }
}

pub(crate) fn service_policy_discovery_scope_from_owner(owner: &str) -> &'static str {
    match owner {
        "persistent" => "global",
        "ephemeral" => "local",
        _ => "",
    }
}

pub(crate) fn service_policy_infer_reuse_policy(
    explicit_reuse: &str,
    owner_scope: &str,
    discovery_scope: &str,
    fallback: &str,
) -> String {
    if !explicit_reuse.is_empty() {
        return explicit_reuse.to_string();
    }

    if owner_scope == "persistent" || discovery_scope == "global" {
        return "same-slot".to_string();
    }

    if owner_scope == "ephemeral" || discovery_scope == "local" {
        return "same-root".to_string();
    }

    fallback.to_string()
}

pub(crate) fn validate_service_policy_reuse_policy(
    reuse: &str,
    allow_empty: bool,
) -> Result<(), String> {
    match reuse {
        "never" | "same-root" | "same-slot" | "cross-run" => Ok(()),
        "" if allow_empty => Ok(()),
        _ => Err(format!(
            "SERVICE_REUSE_POLICY must be one of never|same-root|same-slot|cross-run (got '{}')",
            reuse
        )),
    }
}

pub(crate) fn validate_service_policy_owner_scope(
    owner: &str,
    allow_empty: bool,
) -> Result<(), String> {
    match owner {
        "ephemeral" | "persistent" => Ok(()),
        "" if allow_empty => Ok(()),
        _ => Err(format!(
            "SERVICE_OWNER_SCOPE must be ephemeral|persistent (got '{}')",
            owner
        )),
    }
}

pub(crate) fn validate_service_policy_discovery_scope(
    discovery: &str,
    allow_empty: bool,
) -> Result<(), String> {
    match discovery {
        "local" | "global" => Ok(()),
        "" if allow_empty => Ok(()),
        _ => Err(format!(
            "SERVICE_DISCOVERY_SCOPE must be local|global (got '{}')",
            discovery
        )),
    }
}

pub(crate) fn validate_service_policy_matrix(
    reuse: &str,
    owner: &str,
    discovery: &str,
    allow_empty: bool,
    enforce_owner_discovery_alignment: bool,
) -> Result<(), String> {
    validate_service_policy_reuse_policy(reuse, allow_empty)?;
    validate_service_policy_owner_scope(owner, allow_empty)?;
    validate_service_policy_discovery_scope(discovery, allow_empty)?;

    if reuse == "cross-run" && (owner != "persistent" || discovery != "global") {
        return Err(
            "cross-run reuse requires SERVICE_OWNER_SCOPE=persistent and SERVICE_DISCOVERY_SCOPE=global"
                .to_string(),
        );
    }

    if reuse == "same-root" && (owner != "ephemeral" || discovery != "local") {
        return Err(
            "same-root reuse requires SERVICE_OWNER_SCOPE=ephemeral and SERVICE_DISCOVERY_SCOPE=local"
                .to_string(),
        );
    }

    if enforce_owner_discovery_alignment && !owner.is_empty() && !discovery.is_empty() {
        if owner == "persistent" && discovery != "global" {
            return Err(
                "persistent owner scope requires SERVICE_DISCOVERY_SCOPE=global".to_string(),
            );
        }
        if owner == "ephemeral" && discovery != "local" {
            return Err("ephemeral owner scope requires SERVICE_DISCOVERY_SCOPE=local".to_string());
        }
    }

    Ok(())
}

pub(crate) fn service_policy_start_service_register_cleanup(
    reuse: &str,
    owner: &str,
    discovery: &str,
) -> Result<String, String> {
    match reuse {
        "same-slot" | "cross-run" => Ok("0".to_string()),
        "never" | "same-root" => Ok("1".to_string()),
        "" => {
            if owner == "persistent" || discovery == "global" {
                Ok("0".to_string())
            } else {
                Ok("1".to_string())
            }
        }
        _ => Err(format!("unresolved start_service reuse policy '{}'", reuse)),
    }
}

pub(crate) fn service_policy_fixture_keep_running(
    owner_scope: &str,
    reuse_policy: &str,
    discovery_scope: &str,
) -> Result<String, String> {
    validate_service_policy_owner_scope(owner_scope, true)?;
    validate_service_policy_reuse_policy(reuse_policy, true)?;
    validate_service_policy_discovery_scope(discovery_scope, true)?;

    if owner_scope == "persistent" {
        return Ok("1".to_string());
    }
    if owner_scope == "ephemeral" {
        return Ok("0".to_string());
    }

    match reuse_policy {
        "same-slot" | "cross-run" => return Ok("1".to_string()),
        "never" | "same-root" => return Ok("0".to_string()),
        _ => {}
    }

    match discovery_scope {
        "global" => Ok("1".to_string()),
        "local" => Ok("0".to_string()),
        _ => Ok("0".to_string()),
    }
}
