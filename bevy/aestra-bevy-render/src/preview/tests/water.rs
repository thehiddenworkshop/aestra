use super::*;

#[test]
fn water_uses_dielectric_reflection_and_host_clock_without_a_second_simulation() {
    for state in [SeaState::Calm, SeaState::Rough] {
        let mut water = material(state);
        let f0 = 0.16 * water.base.reflectance.powi(2);
        assert!((f0 - 0.02037).abs() < 0.00001);
        assert_eq!(water.base.metallic, 0.0);
        assert_eq!(water.base.alpha_mode, AlphaMode::Opaque);
        assert!(water.extension.set_time(3.5));
        assert!(!water.extension.set_time(3.5));
        for invalid in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(!water.extension.set_time(invalid));
            assert_eq!(water.extension.time(), 3.5);
        }
        assert!(water.extension.set_time(0.0));
        assert_eq!(water.extension.time(), 0.0);
    }
}
