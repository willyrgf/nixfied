use nixfied_model::{CandidatePortWindow, Model, SlotPlacement};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSlot<'a> {
    pub environment: &'a str,
    pub slot: u32,
    pub placement: &'a SlotPlacement,
}

pub fn select_slot(model: &Model, requested_slot: Option<u32>) -> RuntimeResult<SelectedSlot<'_>> {
    let slot = requested_slot.unwrap_or(model.slot_policy.default);
    if slot < model.slot_policy.min || slot > model.slot_policy.max {
        return Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!(
                "slot {slot} is outside slotPolicy range {}..{}",
                model.slot_policy.min, model.slot_policy.max
            ),
        )
        .with_detail("slot", slot)
        .with_detail("slotMin", model.slot_policy.min)
        .with_detail("slotMax", model.slot_policy.max));
    }
    let key = slot.to_string();
    let placement = model.placement.slot_placements.get(&key).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("model is missing placement for slot {slot}"),
        )
    })?;
    if placement.slot != slot {
        return Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!(
                "placement slot mismatch for key {key}: placement declares {}",
                placement.slot
            ),
        ));
    }
    Ok(SelectedSlot {
        environment: "dev",
        slot,
        placement,
    })
}

pub fn first_candidate_port(window: &CandidatePortWindow) -> RuntimeResult<u16> {
    if window.start == 0 || window.start > window.end {
        return Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!(
                "invalid candidate port window {}-{}",
                window.start, window.end
            ),
        ));
    }
    Ok(window.start)
}
