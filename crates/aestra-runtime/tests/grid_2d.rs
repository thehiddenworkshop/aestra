use aestra_core::ResourceTypeId;
use aestra_runtime::{Grid2dChannels, Grid2dLayout, Grid2dLayoutError, StagedDispatch};
use std::num::NonZeroU32;

#[test]
fn dense_xy_channels_use_the_shared_field_layout_and_dispatch() {
    let grid = Grid2dLayout::new([17, 9], [-19.0, 11.0], 2.0).unwrap();
    for (channels, components, cell_bytes) in [
        (Grid2dChannels::Scalar, 1, 4),
        (Grid2dChannels::Vector2, 2, 8),
        (Grid2dChannels::Vector3, 3, 16),
        (Grid2dChannels::Vector4, 4, 16),
    ] {
        let field = grid.field(ResourceTypeId::new("example.field"), channels);
        assert_eq!(field.dims, [17, 9, 1]);
        assert_eq!(field.origin, [-19.0, 11.0, -1.0]);
        assert_eq!(field.cell_size, 2.0);
        assert_eq!(field.components, components);
        assert_eq!(field.cell_bytes(), cell_bytes);
        assert_eq!(field.stored_cells(), 17 * 9);
        assert!(!field.staggered);
    }
    let workgroup = [8, 4].map(|size| NonZeroU32::new(size).unwrap());
    assert_eq!(
        grid.dispatch(workgroup),
        StagedDispatch { x: 3, y: 3, z: 1 }
    );
}

#[test]
fn grid_dimensions_and_bounds_must_be_representable() {
    for dimensions in [[0, 3], [u32::MAX, 2]] {
        assert_eq!(
            Grid2dLayout::new(dimensions, [0.0; 2], 1.0),
            Err(Grid2dLayoutError::InvalidDimensions)
        );
    }
    for (origin, size) in [
        ([0.0; 2], 0.0),
        ([f32::NAN, 0.0], 1.0),
        ([f32::MAX, 0.0], 1.0),
        ([0.0; 2], f32::MAX),
    ] {
        assert_eq!(
            Grid2dLayout::new([2, 2], origin, size),
            Err(Grid2dLayoutError::InvalidBounds)
        );
    }
}
