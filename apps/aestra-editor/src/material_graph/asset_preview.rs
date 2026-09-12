//! Saved material thumbnails using the same deterministic evaluator as graph previews.
use super::*;

pub(crate) fn render_material_asset_preview(
    program: &MaterialProgram,
    size: u32,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    if cancelled() {
        return Err("Cancelled".into());
    }
    if size == 0 || size > 128 || program.expressions.len() > 256 || program.parameters.len() > 128
    {
        return Err("Preview limit: 128 pixels, 256 expressions and 128 parameters".into());
    }
    if program.outputs.vertex_offset.is_some() {
        return Err("Vertex displacement needs a scene preview".into());
    }
    let expressions: BTreeMap<_, _> = program.expressions.iter().map(|e| (e.id, e)).collect();
    if expressions.len() != program.expressions.len() {
        return Err("Duplicate material expression identity".into());
    }
    // Bound recursion before entering the existing evaluator. Check every reachable branch,
    // including ones not selected at a particular synthetic sample.
    fn visit(
        id: MaterialExpressionId,
        program: &MaterialProgram,
        expressions: &BTreeMap<MaterialExpressionId, &MaterialExpression>,
        active: &mut BTreeSet<MaterialExpressionId>,
        depths: &mut BTreeMap<MaterialExpressionId, usize>,
    ) -> Result<usize, String> {
        if active.len() >= 64 || !active.insert(id) {
            return Err("Material preview contains a cycle or exceeds 64 expression levels".into());
        }
        if let Some(depth) = depths.get(&id) {
            active.remove(&id);
            return Ok(*depth);
        }
        let expression = expressions.get(&id).ok_or("Missing material expression")?;
        let dependencies = if program.disabled_expressions.contains(&id)
            && let Some(source) = expression.kind.bypass_input()
        {
            vec![source]
        } else {
            match expression.kind {
                MaterialExpressionKind::FunctionInput(_)
                | MaterialExpressionKind::FunctionCall { .. }
                | MaterialExpressionKind::CustomWeslCall { .. } => {
                    return Err("Function calls need a compiled scene preview".into());
                }
                MaterialExpressionKind::SampleTexture { .. }
                | MaterialExpressionKind::SampleTextureLevel { .. }
                | MaterialExpressionKind::SampleTextureGradient { .. } => {
                    return Err("Texture sampling needs a bound-texture preview".into());
                }
                MaterialExpressionKind::DerivativeX { .. }
                | MaterialExpressionKind::DerivativeY { .. } => {
                    return Err("Screen derivatives need a scene preview".into());
                }
                _ => {}
            }
            preview_dependencies(&expression.kind)
        };
        let mut depth = 1;
        for dependency in dependencies {
            depth = depth.max(1 + visit(dependency, program, expressions, active, depths)?);
        }
        active.remove(&id);
        if depth > 64 {
            return Err("Preview limit: 64 expression levels".into());
        }
        depths.insert(id, depth);
        Ok(depth)
    }
    let mut depths = BTreeMap::new();
    for root in [program.outputs.color, program.outputs.alpha] {
        visit(
            root,
            program,
            &expressions,
            &mut BTreeSet::new(),
            &mut depths,
        )?;
    }
    render_material_preview_pixels(
        program,
        None,
        MaterialGraphPreviewTarget::Output,
        None,
        size,
        true,
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material() -> MaterialProgram {
        material_preset_base("Preview", MaterialDomain::Sprite)
    }

    #[test]
    fn material_thumbnail_reuses_graph_pixels_without_mutation() {
        let program = material();
        let before = program.clone();
        let pixels = render_material_asset_preview(&program, 128, || false).unwrap();
        assert_eq!(
            pixels,
            render_material_preview_pixels(
                &program,
                None,
                MaterialGraphPreviewTarget::Output,
                None,
                128,
                false,
                || false
            )
            .unwrap()
        );
        assert_eq!(pixels.len(), 128 * 128 * 4);
        assert_ne!(
            &pixels[..4],
            &pixels[(64 * 128 + 64) * 4..(64 * 128 + 64) * 4 + 4]
        );
        assert_eq!(program, before);
    }

    #[test]
    fn unsupported_missing_and_cyclic_outputs_fail_instead_of_fake_previews() {
        let mut program = material();
        let color = program.outputs.color;
        let alpha = program.outputs.alpha;
        let expression = program
            .expressions
            .iter_mut()
            .find(|e| e.id == color)
            .unwrap();
        expression.kind = MaterialExpressionKind::SampleTexture {
            texture: alpha,
            uv: alpha,
        };
        assert!(
            render_material_asset_preview(&program, 32, || false)
                .unwrap_err()
                .contains("Texture sampling")
        );
        // A disconnected unsupported node must not prevent a supported output preview.
        program.outputs.color = alpha;
        assert!(render_material_asset_preview(&program, 32, || false).is_ok());
        program.outputs.color = color;
        program
            .expressions
            .iter_mut()
            .find(|e| e.id == color)
            .unwrap()
            .kind = MaterialExpressionKind::Add(color, alpha);
        assert!(
            render_material_asset_preview(&program, 32, || false)
                .unwrap_err()
                .contains("cycle")
        );
        program.outputs.color = MaterialExpressionId::new();
        assert!(
            render_material_asset_preview(&program, 32, || false)
                .unwrap_err()
                .contains("Missing")
        );
    }

    #[test]
    fn material_preview_limits_depth_size_and_cancels_between_rows() {
        let mut program = material();
        assert!(render_material_asset_preview(&program, 129, || false).is_err());
        assert!(render_material_asset_preview(&program, 0, || false).is_err());
        let checks = std::cell::Cell::new(0);
        assert_eq!(
            render_material_asset_preview(&program, 128, || {
                checks.set(checks.get() + 1);
                checks.get() > 3
            })
            .unwrap_err(),
            "Cancelled"
        );
        for _ in 0..65 {
            let id = MaterialExpressionId::new();
            program.expressions.push(MaterialExpression {
                id,
                kind: MaterialExpressionKind::Add(program.outputs.color, program.outputs.alpha),
            });
            program.outputs.color = id;
        }
        assert!(
            render_material_asset_preview(&program, 32, || false)
                .unwrap_err()
                .contains("64")
        );
        while program.expressions.len() <= 256 {
            program.expressions.push(program.expressions[0].clone());
        }
        assert!(
            render_material_asset_preview(&program, 32, || false)
                .unwrap_err()
                .contains("256")
        );
    }
}
