// Aestra Fluid — the dense grid (fluid F7): every cell of the N³ grid is stored, x fastest, and a pass
// covers the grid in 4³ workgroups, so an invocation's global id is its cell. Composed with the solver;
// `grid_sparse.wgsl` is the same interface over sparse bricks.

fn cell_index(cell: vec3<u32>) -> u32 {
    let n = grid_res();
    return (cell.z * n + cell.y) * n + cell.x;
}

// The cell an invocation of a grid pass works on.
fn grid_cell(gid: vec3<u32>) -> vec3<u32> {
    return gid;
}

// Whether the grid stores `cell` (inside it): always.
fn cell_active(cell: vec3<i32>) -> bool {
    return true;
}

// The cell an invocation of a pass over multigrid level `level` works on.
fn lv_cell(level: u32, gid: vec3<u32>) -> vec3<u32> {
    return gid;
}

// Where level `level` starts in the per-level buffers.
fn lv_base(level: u32) -> u32 {
    var n = grid_res();
    var base = 0u;
    for (var l = 0u; l < level; l += 1u) {
        base += n * n * n;
        n = n / 2u;
    }
    return base;
}

// The index of cell `c` of level `level` (inside the level) in the per-level buffers.
fn lv_slot(level: u32, c: vec3<i32>) -> i32 {
    let n = mg_level_res(level);
    let cell = vec3<u32>(c);
    return i32(lv_base(level) + (cell.z * n + cell.y) * n + cell.x);
}

// The workgroups a fine-grid pass runs, and one's place among them (for per-workgroup partial sums).
fn grid_group_count() -> u32 {
    let g = grid_res() / 4u;
    return g * g * g;
}

fn grid_group_index(group: vec3<u32>) -> u32 {
    let g = grid_res() / 4u;
    return (group.z * g + group.y) * g + group.x;
}

// Whether no side of the stored region is open: its pressure is then defined only up to a constant.
fn grid_fully_closed() -> bool {
    return open_sides() == 0u;
}
