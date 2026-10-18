//! Actual Curves controls' action payloads; native consumers remain in the Curves plugin.
use aestra_core::ModuleId;
use bevy::prelude::Component;
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurvesAction {
    OpenHost(u8),
    HostAddKey,
    HostDeleteKey,
    OpenInput(ModuleId, u8),
    SelectVectorChannel(u8),
    SetInterpolation(aestra_core::CurveInterpolation),
    AddKey,
    DeleteKey,
    AdjustTime(i8),
    AdjustCurveValue(i8),
    AdjustGradientChannel(u8, i8),
}
