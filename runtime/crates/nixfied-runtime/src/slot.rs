use nixfied_manifest::{CandidatePortWindow, Manifest, SlotPlacement};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSlot<'a> {
    pub environment: &'a str,
    pub slot: u32,
    pub placement: &'a SlotPlacement,
}

pub fn select_slot(
    manifest: &Manifest,
    requested_slot: Option<u32>,
) -> RuntimeResult<SelectedSlot<'_>> {
    let slot = requested_slot.unwrap_or(manifest.slot_policy.default);
    if slot < manifest.slot_policy.min || slot > manifest.slot_policy.max {
        return Err(RuntimeError::new(
            ErrorCode::ManifestAdmission,
            format!(
                "slot {slot} is outside slotPolicy range {}..{}",
                manifest.slot_policy.min, manifest.slot_policy.max
            ),
        )
        .with_detail("slot", slot)
        .with_detail("slotMin", manifest.slot_policy.min)
        .with_detail("slotMax", manifest.slot_policy.max));
    }
    let key = slot.to_string();
    let placement = manifest
        .placement
        .slot_placements
        .get(&key)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ManifestAdmission,
                format!("manifest is missing placement for slot {slot}"),
            )
        })?;
    if placement.slot != slot {
        return Err(RuntimeError::new(
            ErrorCode::ManifestAdmission,
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
            ErrorCode::ManifestAdmission,
            format!(
                "invalid candidate port window {}-{}",
                window.start, window.end
            ),
        ));
    }
    Ok(window.start)
}
