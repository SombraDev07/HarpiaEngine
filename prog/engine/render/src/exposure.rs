//! Auto-exposure: middle-grey mapping of average Rec.709 luminance.
//!
//! The GPU writes a 1×1 R32; the tonemap / fog apply multiplies by it.
//! Adaptation is the usual exponential (`1 - exp(-dt * speed)`).

pub const MIDDLE_GREY: f32 = 0.18;
pub const ADAPT_SPEED: f32 = 1.2;
pub const EXPOSURE_MIN: f32 = 0.05;
pub const EXPOSURE_MAX: f32 = 12.0;
/// 1×1 R32 UAV slot. SPD uses 0..N, SSSR uses 15.
pub const EXPOSURE_UAV_SLOT: u32 = 14;

pub fn target_from_luma(avg: f32) -> f32 {
    let t = MIDDLE_GREY / avg.max(1e-4);
    t.clamp(EXPOSURE_MIN, EXPOSURE_MAX)
}

pub fn adapt(current: f32, target: f32, dt: f32) -> f32 {
    let w = 1.0 - (-dt * ADAPT_SPEED).exp();
    current + (target - current) * w.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hot_scene_pulls_exposure_down() {
        let hot = target_from_luma(4.0);
        let dark = target_from_luma(0.02);
        assert!(hot < 1.0, "bright average must stop clipping, got {hot}");
        assert!(dark > 1.0, "dark average must lift, got {dark}");
        let mut e = 1.0;
        for _ in 0..60 {
            e = adapt(e, hot, 1.0 / 60.0);
        }
        assert!(
            e < 0.55 && e < 1.0,
            "after a second at 60 Hz exposure is {e}, should have dropped toward {hot}"
        );
    }
}
