//! Bounded function dependency walk for effect thumbnails and hover scenes.
//! Only canonical built-in noise and the existing source-verified smoke kernel
//! can introduce shader code. Project graph wrappers remain ordinary typed math.
use aestra_core::{
    MaterialFunctionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionRef,
        MaterialProgram,
    },
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn effect_dependencies(
    program: &MaterialProgram,
    functions: &BTreeMap<MaterialFunctionId, MaterialFunction>,
) -> Option<BTreeSet<MaterialFunctionId>> {
    struct Walk<'a> {
        functions: &'a BTreeMap<MaterialFunctionId, MaterialFunction>,
        used: BTreeSet<MaterialFunctionId>,
        active: BTreeSet<MaterialFunctionId>,
        expressions: usize,
        calls: usize,
        kernels: usize,
    }
    impl Walk<'_> {
        fn visit(&mut self, expressions: &[MaterialExpression], depth: usize) -> Option<()> {
            self.expressions += expressions.len();
            if depth > 8 || self.expressions > 512 {
                return None;
            }
            for expression in expressions {
                match &expression.kind {
                    MaterialExpressionKind::CustomWeslCall { .. } => return None,
                    MaterialExpressionKind::FunctionInput(_) if depth == 0 => return None,
                    MaterialExpressionKind::FunctionCall { function, .. } => {
                        self.calls += 1;
                        if self.calls > 16 {
                            return None;
                        }
                        match function {
                            MaterialFunctionRef::BuiltIn(id) => {
                                aestra_core::noise::NoiseFunction::from_id(*id)?;
                                // Built-ins come from the compiler's canonical registry, never
                                // from project definitions that happen to reuse the same UUID.
                                self.kernels += 1;
                            }
                            MaterialFunctionRef::Project(id) => {
                                if !self.active.insert(*id) {
                                    return None;
                                }
                                let function = self.functions.get(id)?;
                                self.used.insert(*id);
                                if super::material::shared_smoke_function(function) {
                                    self.kernels += 1;
                                } else if function.custom_wesl.is_some() {
                                    return None;
                                } else {
                                    self.visit(&function.expressions, depth + 1)?;
                                }
                                self.active.remove(id);
                            }
                        }
                        if self.kernels > 4 {
                            return None;
                        }
                    }
                    _ => {}
                }
            }
            Some(())
        }
    }
    let mut walk = Walk {
        functions,
        used: BTreeSet::new(),
        active: BTreeSet::new(),
        expressions: 0,
        calls: 0,
        kernels: 0,
    };
    walk.visit(&program.expressions, 0)?;
    if walk.calls > 0 && walk.kernels == 0 {
        return None;
    }
    Some(walk.used)
}
