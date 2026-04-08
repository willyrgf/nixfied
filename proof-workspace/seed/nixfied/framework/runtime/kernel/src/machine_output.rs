use super::*;

pub(crate) fn machine_output_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "run" => machine_output_run_command(values),
        other => Err(format!("unknown machine-output subcommand: {}", other)),
    }
}

fn machine_output_run_command(values: &[String]) -> Result<(), String> {
    let plan_path = values.first().ok_or_else(|| {
        "usage: nixfied-kernel machine-output run <plan-file> [-- <args...>]".to_string()
    })?;
    let plan = load_machine_output_plan(plan_path)?;
    let remaining = values[1..].to_vec();
    let user_args = strip_passthrough_separator(&remaining).to_vec();
    let work_dir = create_temp_dir("nixfied-machine-output")?;

    for (index, setup_program) in plan.setup_programs.iter().enumerate() {
        let output = run_captured_program(setup_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs("INFO", &format!("setup app {}", index + 1), &output);
        } else {
            render_captured_logs("ERROR", &format!("setup app {}", index + 1), &output);
            machine_output_fail(
                &plan,
                "setup",
                "machine-output-setup-failed",
                &format!("setup app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    let payload_file = format!("{}/payload.json", work_dir);
    let mut target_args = plan.target_args.clone();
    target_args.extend(user_args.iter().cloned());
    let output = run_captured_program(
        &plan.target_program,
        &target_args,
        &[(
            "NIXFIED_MACHINE_OUTPUT_FILE".to_string(),
            payload_file.clone(),
        )],
    )?;
    if output.status.success() {
        render_captured_logs("INFO", "target app", &output);
    } else {
        render_captured_logs("ERROR", "target app", &output);
        machine_output_fail(
            &plan,
            "target",
            "machine-output-target-failed",
            &format!("target app '{}' failed", plan.target_app_id),
            &plan.target_app_id,
            output.status.code().unwrap_or(1),
        );
    }

    let payload_text = match fs::read_to_string(&payload_file) {
        Ok(text) if !text.trim().is_empty() => text,
        _ => {
            machine_output_fail(
                &plan,
                "validation",
                "machine-output-validation-failed",
                &format!(
                    "target app '{}' did not write machine payload to declared file",
                    plan.target_app_id
                ),
                &plan.target_app_id,
                1,
            );
        }
    };
    let payload = parse_json(&payload_text).unwrap_or_else(|_| {
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    });
    let bundle = parse_json_file(&plan.bundle_file, "machine-output validation bundle")
        .unwrap_or_else(|err| panic!("{}", err));
    if let Err(err) = validate_json_value_against_contract(&bundle, &plan.contract_ref, &payload) {
        eprintln!("ERROR: {}", err);
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    }

    for (index, teardown_program) in plan.teardown_programs.iter().enumerate() {
        let output = run_captured_program(teardown_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs("INFO", &format!("teardown app {}", index + 1), &output);
        } else {
            render_captured_logs("ERROR", &format!("teardown app {}", index + 1), &output);
            machine_output_fail(
                &plan,
                "teardown",
                "machine-output-teardown-failed",
                &format!("teardown app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    print!("{}", payload_text);
    Ok(())
}

fn load_machine_output_plan(path: &str) -> Result<MachineOutputPlan, String> {
    let value = parse_json_file(path, "machine-output plan")?;
    Ok(MachineOutputPlan {
        app_id: required_string_field(&value, "appId", "machine-output plan")?.to_string(),
        target_app_id: required_string_field(&value, "targetAppId", "machine-output plan")?
            .to_string(),
        contract_ref: required_string_field(&value, "contractRef", "machine-output plan")?
            .to_string(),
        bundle_file: required_string_field(&value, "bundleFile", "machine-output plan")?
            .to_string(),
        target_program: required_string_field(&value, "targetProgram", "machine-output plan")?
            .to_string(),
        setup_programs: array_strings(&value, "setupPrograms"),
        teardown_programs: array_strings(&value, "teardownPrograms"),
        target_args: array_strings(&value, "targetArgs"),
    })
}

fn machine_output_fail(
    plan: &MachineOutputPlan,
    stage: &str,
    code: &str,
    message: &str,
    failed_app_id: &str,
    exit_code: i32,
) -> ! {
    let failed_app_id_val = nullable_string_value(failed_app_id);
    let contract_ref_val = nullable_string_value(&plan.contract_ref);
    let validator_val: JsonValue = if stage == "validation" {
        json!("nixfied-kernel")
    } else {
        JsonValue::Null
    };
    let payload = json!({
        "ok": false,
        "appId": plan.app_id,
        "targetAppId": plan.target_app_id,
        "stage": stage,
        "code": code,
        "message": message,
        "failedAppId": failed_app_id_val,
        "contractRef": contract_ref_val,
        "validator": validator_val,
        "exitCode": exit_code as i64,
    });
    println!("{}", render_json_compact(&payload));
    process::exit(exit_code.max(1));
}
